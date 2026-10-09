//! 本地凭据保管：OS Keychain 优先，AES-GCM 加密文件兜底。
//!
//! Keychain 不可用（Linux 无 Secret Service、受限桌面会话等）时，使用配置目录
//! 内随机 256-bit 主密钥加密 secrets.json。主密钥和密文文件在 Unix 均限制为 0600。

use aes_gcm::{
    aead::{Aead, KeyInit, OsRng},
    Aes256Gcm, Nonce,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use rand::RngCore;
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const SERVICE: &str = "net.qihoo.pubsec.easys3";

pub struct SecretStore {
    dir: PathBuf,
}

impl SecretStore {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub fn get(&self, id: &str) -> Result<Option<String>, String> {
        if let Ok(entry) = keyring::Entry::new(SERVICE, id) {
            match entry.get_password() {
                Ok(secret) => return Ok(Some(secret)),
                Err(keyring::Error::NoEntry) => {}
                Err(_) => {}
            }
        }
        self.get_fallback(id)
    }

    pub fn set(&self, id: &str, secret: &str) -> Result<(), String> {
        if let Ok(entry) = keyring::Entry::new(SERVICE, id) {
            if entry.set_password(secret).is_ok() {
                // Keychain 成功后无需保留过期的兜底副本。
                let _ = self.remove_fallback(id);
                return Ok(());
            }
        }
        self.set_fallback(id, secret)
    }

    pub fn delete(&self, id: &str) {
        if let Ok(entry) = keyring::Entry::new(SERVICE, id) {
            let _ = entry.delete_credential();
        }
        let _ = self.remove_fallback(id);
    }

    fn key_path(&self) -> PathBuf {
        self.dir.join("secrets.key")
    }

    fn data_path(&self) -> PathBuf {
        self.dir.join("secrets.json")
    }

    fn master_key(&self) -> Result<[u8; 32], String> {
        let path = self.key_path();
        match fs::read(&path) {
            Ok(bytes) if bytes.len() == 32 => {
                let mut key = [0; 32];
                key.copy_from_slice(&bytes);
                Ok(key)
            }
            Ok(_) => Err("本地凭据主密钥无效，请重新保存连接密钥".to_string()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                fs::create_dir_all(&self.dir).map_err(|e| format!("创建凭据目录失败：{e}"))?;
                let mut key = [0; 32];
                OsRng.fill_bytes(&mut key);
                fs::write(&path, key).map_err(|e| format!("写入凭据主密钥失败：{e}"))?;
                restrict_file(&path);
                Ok(key)
            }
            Err(error) => Err(format!("读取凭据主密钥失败：{error}")),
        }
    }

    fn read_fallback(&self) -> Result<BTreeMap<String, String>, String> {
        let path = self.data_path();
        match fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text).map_err(|_| "本地加密凭据文件损坏".to_string()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(BTreeMap::new()),
            Err(error) => Err(format!("读取加密凭据失败：{error}")),
        }
    }

    fn write_fallback(&self, data: &BTreeMap<String, String>) -> Result<(), String> {
        fs::create_dir_all(&self.dir).map_err(|e| format!("创建凭据目录失败：{e}"))?;
        let path = self.data_path();
        let tmp = path.with_extension("tmp");
        let text = serde_json::to_string(data).map_err(|e| format!("序列化加密凭据失败：{e}"))?;
        fs::write(&tmp, text).map_err(|e| format!("写入加密凭据失败：{e}"))?;
        restrict_file(&tmp);
        fs::rename(&tmp, path).map_err(|e| format!("保存加密凭据失败：{e}"))
    }

    fn get_fallback(&self, id: &str) -> Result<Option<String>, String> {
        let Some(encoded) = self.read_fallback()?.get(id).cloned() else {
            return Ok(None);
        };
        let bytes = STANDARD
            .decode(encoded)
            .map_err(|_| "加密凭据编码无效".to_string())?;
        if bytes.len() <= 12 {
            return Err("加密凭据内容无效".to_string());
        }
        let key = self.master_key()?;
        let cipher =
            Aes256Gcm::new_from_slice(&key).map_err(|_| "初始化凭据加密器失败".to_string())?;
        let plain = cipher
            .decrypt(Nonce::from_slice(&bytes[..12]), &bytes[12..])
            .map_err(|_| "无法解密本地凭据".to_string())?;
        String::from_utf8(plain)
            .map(Some)
            .map_err(|_| "本地凭据不是有效文本".to_string())
    }

    fn set_fallback(&self, id: &str, secret: &str) -> Result<(), String> {
        let key = self.master_key()?;
        let cipher =
            Aes256Gcm::new_from_slice(&key).map_err(|_| "初始化凭据加密器失败".to_string())?;
        let mut nonce = [0; 12];
        OsRng.fill_bytes(&mut nonce);
        let ciphertext = cipher
            .encrypt(Nonce::from_slice(&nonce), secret.as_bytes())
            .map_err(|_| "加密凭据失败".to_string())?;
        let mut bytes = nonce.to_vec();
        bytes.extend(ciphertext);
        let mut data = self.read_fallback()?;
        data.insert(id.to_string(), STANDARD.encode(bytes));
        self.write_fallback(&data)
    }

    fn remove_fallback(&self, id: &str) -> Result<(), String> {
        let mut data = self.read_fallback()?;
        if data.remove(id).is_some() {
            self.write_fallback(&data)?;
        }
        Ok(())
    }
}

fn restrict_file(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aes_fallback_round_trip_and_removal() {
        let dir = tempfile::tempdir().unwrap();
        let store = SecretStore::new(dir.path().to_owned());
        store
            .set_fallback("connection-id", "not-in-plain-text")
            .unwrap();
        assert_eq!(
            store.get_fallback("connection-id").unwrap().as_deref(),
            Some("not-in-plain-text")
        );
        let contents = fs::read_to_string(store.data_path()).unwrap();
        assert!(!contents.contains("not-in-plain-text"));
        store.remove_fallback("connection-id").unwrap();
        assert!(store.get_fallback("connection-id").unwrap().is_none());
    }
}
