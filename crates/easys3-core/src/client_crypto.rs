//! 客户端加密对象格式（P2-01）。
//! 每个分块独立 AES-256-GCM 认证，读取与写入均为顺序流式，绝不整文件入内存。

use crate::error::CoreError;
use aes_gcm::{
    aead::{Aead, KeyInit, OsRng},
    Aes256Gcm, Nonce,
};
use rand::RngCore;
use std::path::Path;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub const MAGIC: &[u8; 4] = b"ES3C";
pub const VERSION: u8 = 1;
pub const CHUNK_SIZE: usize = 1024 * 1024;
const HEADER_SIZE: usize = 4 + 1 + 4 + 8 + 8;

#[derive(Clone, Debug)]
pub struct ClientEncryptionKey(pub [u8; 32]);

impl ClientEncryptionKey {
    pub fn from_base64(value: &str) -> Result<Self, CoreError> {
        use base64::{engine::general_purpose::STANDARD, Engine};
        let bytes = STANDARD
            .decode(value)
            .map_err(|_| CoreError::Other("客户端加密密钥必须是 Base64".into()))?;
        let key: [u8; 32] = bytes
            .try_into()
            .map_err(|_| CoreError::Other("客户端加密密钥解码后必须为 32 字节".into()))?;
        Ok(Self(key))
    }
}

fn cipher(key: &ClientEncryptionKey) -> Aes256Gcm {
    Aes256Gcm::new_from_slice(&key.0).expect("AES-256 key length")
}
fn nonce(prefix: &[u8; 8], index: u32) -> [u8; 12] {
    let mut value = [0u8; 12];
    value[..8].copy_from_slice(prefix);
    value[8..].copy_from_slice(&index.to_be_bytes());
    value
}

/// 返回加密后的字节数。输出格式：header + (ciphertext length u32 + ciphertext)*。
pub async fn encrypt_file(
    input: &Path,
    output: &Path,
    key: &ClientEncryptionKey,
) -> Result<u64, CoreError> {
    let plaintext_size = tokio::fs::metadata(input)
        .await
        .map_err(|e| CoreError::io(format!("读取文件失败 {}", input.display()), e))?
        .len();
    let mut source = tokio::fs::File::open(input)
        .await
        .map_err(|e| CoreError::io(format!("打开文件失败 {}", input.display()), e))?;
    let mut target = tokio::fs::File::create(output)
        .await
        .map_err(|e| CoreError::io(format!("创建加密临时文件失败 {}", output.display()), e))?;
    let mut prefix = [0u8; 8];
    OsRng.fill_bytes(&mut prefix);
    target
        .write_all(MAGIC)
        .await
        .map_err(|e| CoreError::io("写入加密对象头失败", e))?;
    target
        .write_all(&[VERSION])
        .await
        .map_err(|e| CoreError::io("写入加密对象头失败", e))?;
    target
        .write_all(&(CHUNK_SIZE as u32).to_be_bytes())
        .await
        .map_err(|e| CoreError::io("写入加密对象头失败", e))?;
    target
        .write_all(&plaintext_size.to_be_bytes())
        .await
        .map_err(|e| CoreError::io("写入加密对象头失败", e))?;
    target
        .write_all(&prefix)
        .await
        .map_err(|e| CoreError::io("写入加密对象头失败", e))?;
    let mut buffer = vec![0; CHUNK_SIZE];
    let mut index = 0u32;
    let mut written = HEADER_SIZE as u64;
    loop {
        let count = source
            .read(&mut buffer)
            .await
            .map_err(|e| CoreError::io("读取待加密文件失败", e))?;
        if count == 0 {
            break;
        }
        let n = nonce(&prefix, index);
        let encrypted = cipher(key)
            .encrypt(Nonce::from_slice(&n), &buffer[..count])
            .map_err(|_| CoreError::Other("客户端加密失败".into()))?;
        let length = u32::try_from(encrypted.len())
            .map_err(|_| CoreError::Other("客户端加密分块过大".into()))?;
        target
            .write_all(&length.to_be_bytes())
            .await
            .map_err(|e| CoreError::io("写入加密临时文件失败", e))?;
        target
            .write_all(&encrypted)
            .await
            .map_err(|e| CoreError::io("写入加密临时文件失败", e))?;
        written += 4 + encrypted.len() as u64;
        index = index
            .checked_add(1)
            .ok_or_else(|| CoreError::Other("客户端加密对象分块数超过上限".into()))?;
    }
    target
        .flush()
        .await
        .map_err(|e| CoreError::io("写入加密临时文件失败", e))?;
    Ok(written)
}

pub async fn decrypt_file(
    input: &Path,
    output: &Path,
    key: &ClientEncryptionKey,
) -> Result<u64, CoreError> {
    let mut source = tokio::fs::File::open(input)
        .await
        .map_err(|e| CoreError::io(format!("打开加密对象失败 {}", input.display()), e))?;
    let mut header = [0; HEADER_SIZE];
    source
        .read_exact(&mut header)
        .await
        .map_err(|_| CoreError::Other("客户端加密对象格式无效或已截断".into()))?;
    if &header[..4] != MAGIC || header[4] != VERSION {
        return Err(CoreError::Other("不是支持的客户端加密对象格式".into()));
    }
    let chunk_size = u32::from_be_bytes(header[5..9].try_into().expect("slice length")) as usize;
    if chunk_size != CHUNK_SIZE {
        return Err(CoreError::Other("客户端加密对象分块大小不受支持".into()));
    }
    let expected = u64::from_be_bytes(header[9..17].try_into().expect("slice length"));
    let prefix: [u8; 8] = header[17..25].try_into().expect("slice length");
    let mut target = tokio::fs::File::create(output)
        .await
        .map_err(|e| CoreError::io(format!("创建解密文件失败 {}", output.display()), e))?;
    let mut total = 0u64;
    let mut index = 0u32;
    loop {
        let mut length = [0; 4];
        match source.read_exact(&mut length).await {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(CoreError::io("读取加密对象失败", e)),
        }
        let length = u32::from_be_bytes(length) as usize;
        if !(16..=CHUNK_SIZE + 16).contains(&length) {
            return Err(CoreError::Other("客户端加密对象分块长度无效".into()));
        }
        let mut encrypted = vec![0; length];
        source
            .read_exact(&mut encrypted)
            .await
            .map_err(|_| CoreError::Other("客户端加密对象已截断".into()))?;
        let n = nonce(&prefix, index);
        let plain = cipher(key)
            .decrypt(Nonce::from_slice(&n), encrypted.as_ref())
            .map_err(|_| CoreError::Other("客户端加密对象认证失败：密钥错误或内容被篡改".into()))?;
        target
            .write_all(&plain)
            .await
            .map_err(|e| CoreError::io("写入解密文件失败", e))?;
        total += plain.len() as u64;
        index = index
            .checked_add(1)
            .ok_or_else(|| CoreError::Other("客户端加密对象分块数超过上限".into()))?;
    }
    target
        .flush()
        .await
        .map_err(|e| CoreError::io("写入解密文件失败", e))?;
    if total != expected {
        return Err(CoreError::Other("客户端加密对象长度校验失败".into()));
    }
    Ok(total)
}

/// 预览等受大小上限约束的小对象可在内存认证解密；传输任务必须使用 `decrypt_file`。
pub fn decrypt_bytes(bytes: &[u8], key: &ClientEncryptionKey) -> Result<Vec<u8>, CoreError> {
    if bytes.len() < HEADER_SIZE || &bytes[..4] != MAGIC || bytes[4] != VERSION {
        return Err(CoreError::Other("不是支持的客户端加密对象格式".into()));
    }
    let chunk_size = u32::from_be_bytes(bytes[5..9].try_into().expect("slice length")) as usize;
    if chunk_size != CHUNK_SIZE {
        return Err(CoreError::Other("客户端加密对象分块大小不受支持".into()));
    }
    let expected = u64::from_be_bytes(bytes[9..17].try_into().expect("slice length"));
    let prefix: [u8; 8] = bytes[17..25].try_into().expect("slice length");
    let mut position = HEADER_SIZE;
    let mut index = 0u32;
    let mut output = Vec::with_capacity(usize::try_from(expected).unwrap_or(0));
    while position < bytes.len() {
        if bytes.len().saturating_sub(position) < 4 {
            return Err(CoreError::Other("客户端加密对象已截断".into()));
        }
        let length = u32::from_be_bytes(
            bytes[position..position + 4]
                .try_into()
                .expect("slice length"),
        ) as usize;
        position += 4;
        if !(16..=CHUNK_SIZE + 16).contains(&length)
            || bytes.len().saturating_sub(position) < length
        {
            return Err(CoreError::Other("客户端加密对象分块无效或已截断".into()));
        }
        let n = nonce(&prefix, index);
        let plain = cipher(key)
            .decrypt(Nonce::from_slice(&n), &bytes[position..position + length])
            .map_err(|_| CoreError::Other("客户端加密对象认证失败：密钥错误或内容被篡改".into()))?;
        output.extend_from_slice(&plain);
        position += length;
        index = index
            .checked_add(1)
            .ok_or_else(|| CoreError::Other("客户端加密对象分块数超过上限".into()))?;
    }
    if output.len() as u64 != expected {
        return Err(CoreError::Other("客户端加密对象长度校验失败".into()));
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn roundtrip_multichunk() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        let encrypted = dir.path().join("encrypted");
        let result = dir.path().join("result");
        let data = (0..CHUNK_SIZE + 17)
            .map(|n| (n % 251) as u8)
            .collect::<Vec<_>>();
        tokio::fs::write(&source, &data).await.unwrap();
        let key = ClientEncryptionKey([7; 32]);
        encrypt_file(&source, &encrypted, &key).await.unwrap();
        decrypt_file(&encrypted, &result, &key).await.unwrap();
        assert_eq!(tokio::fs::read(result).await.unwrap(), data);
    }
}
