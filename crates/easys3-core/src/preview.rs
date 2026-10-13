//! 对象预览：图片、PDF、常见音视频与 UTF-8 文本；SVG 一律不预览（webview XSS 风险）。
//! HTML 不渲染（防 XSS）；PDF / 音视频以 data URL 交由前端内嵌展示，不经临时文件。
//! 规格：`.agents/s3-operations.md`「5. 对象预览」。

use crate::client_crypto::{decrypt_bytes, ClientEncryptionKey};
use crate::download::SseCustomerKey;
use crate::error::{classify, CoreError};
use aws_sdk_s3::Client;
use base64::Engine;
use serde::Serialize;
use tokio::io::AsyncReadExt;

pub const TEXT_MAX_BYTES: i64 = 1024 * 1024;
pub const IMAGE_MAX_BYTES: i64 = 20 * 1024 * 1024;
/// PDF / 音视频默认大小上限；设置页可配（TransferSettings.preview_limit_mb）
pub const DEFAULT_PREVIEW_LIMIT_MB: i64 = 20;

const IMAGE_EXTS: &[&str] = &["jpg", "jpeg", "png", "gif", "webp", "bmp"];

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PreviewData {
    Text {
        content: String,
    },
    Image {
        mime: String,
        data_base64: String,
    },
    Pdf {
        data_base64: String,
    },
    /// 视频与音频统一承载，前端按 mime 前缀选择 <video> / <audio>
    Media {
        mime: String,
        data_base64: String,
    },
}

fn ext_of(key: &str) -> String {
    match key.rsplit_once('.') {
        Some((_, ext)) => ext.to_ascii_lowercase(),
        None => String::new(),
    }
}

fn image_mime(ext: &str) -> &'static str {
    match ext {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        _ => "application/octet-stream",
    }
}

fn media_mime(ext: &str) -> Option<&'static str> {
    match ext {
        "pdf" => Some("application/pdf"),
        "mp4" | "m4v" => Some("video/mp4"),
        "webm" => Some("video/webm"),
        "mp3" => Some("audio/mpeg"),
        "wav" => Some("audio/wav"),
        "ogg" => Some("audio/ogg"),
        "flac" => Some("audio/flac"),
        _ => None,
    }
}

/// 预览对象。超限 / 二进制 / SVG 返回可读错误提示改用下载。
pub async fn preview_object(
    client: &Client,
    bucket: &str,
    key: &str,
    media_limit_bytes: i64,
) -> Result<PreviewData, CoreError> {
    preview_object_with_sse(client, bucket, key, media_limit_bytes, None).await
}

pub async fn preview_object_with_sse(
    client: &Client,
    bucket: &str,
    key: &str,
    media_limit_bytes: i64,
    sse: Option<&SseCustomerKey>,
) -> Result<PreviewData, CoreError> {
    preview_object_with_encryption(client, bucket, key, media_limit_bytes, sse, None).await
}

pub async fn preview_object_with_encryption(
    client: &Client,
    bucket: &str,
    key: &str,
    media_limit_bytes: i64,
    sse: Option<&SseCustomerKey>,
    client_encryption: Option<&ClientEncryptionKey>,
) -> Result<PreviewData, CoreError> {
    let ext = ext_of(key);
    if ext == "svg" {
        return Err(CoreError::Other(
            "SVG 文件不支持预览，请下载后查看".to_string(),
        ));
    }

    // 先 Head 拿大小，超限直接拒绝，避免无谓下载
    let head_request = client.head_object().bucket(bucket).key(key);
    let head = if let Some(sse) = sse {
        head_request
            .sse_customer_algorithm("AES256")
            .sse_customer_key(&sse.key_base64)
            .sse_customer_key_md5(&sse.key_md5_base64)
            .send()
            .await
    } else {
        head_request.send().await
    }
    .map_err(|e| classify(&e))?;
    let is_client_encrypted = head
        .metadata()
        .and_then(|metadata| metadata.get("easys3-crypto"))
        .is_some_and(|value| value == "aes256-gcm-v1");
    if is_client_encrypted && client_encryption.is_none() {
        return Err(CoreError::Other(
            "对象使用 easyS3 客户端 AES-256 加密，请先设置客户端 AES-256 会话密钥".into(),
        ));
    }
    let len = if is_client_encrypted {
        head.metadata()
            .and_then(|metadata| metadata.get("easys3-crypto-plain-length"))
            .and_then(|value| value.parse::<i64>().ok())
            .unwrap_or(0)
    } else {
        head.content_length().unwrap_or(0)
    };

    let is_image = IMAGE_EXTS.contains(&ext.as_str());
    let is_media = media_mime(&ext).is_some();
    if is_image && len > IMAGE_MAX_BYTES {
        return Err(CoreError::Other("图片超过 20MB，请下载后查看".to_string()));
    }
    if is_media && len > media_limit_bytes {
        return Err(CoreError::Other(format!(
            "该类型预览上限 {}MB，请下载后查看",
            media_limit_bytes / 1024 / 1024
        )));
    }
    if !is_image && !is_media && len > TEXT_MAX_BYTES {
        return Err(CoreError::Other("文本超过 1MB，请下载后查看".to_string()));
    }

    let get_request = client.get_object().bucket(bucket).key(key);
    let out = if let Some(sse) = sse {
        get_request
            .sse_customer_algorithm("AES256")
            .sse_customer_key(&sse.key_base64)
            .sse_customer_key_md5(&sse.key_md5_base64)
            .send()
            .await
    } else {
        get_request.send().await
    }
    .map_err(|e| classify(&e))?;
    let mut bytes = Vec::new();
    out.body
        .into_async_read()
        .read_to_end(&mut bytes)
        .await
        .map_err(|e| CoreError::io("读取对象失败", e))?;
    let bytes = if let Some(encryption) = client_encryption.filter(|_| is_client_encrypted) {
        decrypt_bytes(&bytes, encryption)?
    } else {
        bytes
    };

    if is_image {
        Ok(PreviewData::Image {
            mime: image_mime(&ext).to_string(),
            data_base64: base64::engine::general_purpose::STANDARD.encode(&bytes),
        })
    } else if let Some(mime) = media_mime(&ext) {
        Ok(PreviewData::Media {
            mime: mime.to_string(),
            data_base64: base64::engine::general_purpose::STANDARD.encode(&bytes),
        })
    } else {
        match String::from_utf8(bytes) {
            Ok(text) => Ok(PreviewData::Text { content: text }),
            Err(_) => Err(CoreError::Other("二进制文件，请下载后查看".to_string())),
        }
    }
}

/// 网格缩略图复用安全图片预览限制；非图片/超限返回普通预览错误，不读取完整大对象。
pub async fn thumbnail_object(
    client: &Client,
    bucket: &str,
    key: &str,
) -> Result<PreviewData, CoreError> {
    thumbnail_object_with_encryption(client, bucket, key, None, None).await
}

pub async fn thumbnail_object_with_encryption(
    client: &Client,
    bucket: &str,
    key: &str,
    sse: Option<&SseCustomerKey>,
    client_encryption: Option<&ClientEncryptionKey>,
) -> Result<PreviewData, CoreError> {
    match preview_object_with_encryption(
        client,
        bucket,
        key,
        20 * 1024 * 1024,
        sse,
        client_encryption,
    )
    .await?
    {
        data @ PreviewData::Image { .. } => Ok(data),
        _ => Err(CoreError::Other("该对象不是可生成缩略图的图片".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ext_extraction() {
        assert_eq!(ext_of("a/b/c.PNG"), "png");
        assert_eq!(ext_of("noext"), "");
        assert_eq!(ext_of("中文.Svg"), "svg");
    }

    #[test]
    fn image_mime_map() {
        assert_eq!(image_mime("jpg"), "image/jpeg");
        assert_eq!(image_mime("webp"), "image/webp");
    }

    #[test]
    fn media_mime_map() {
        assert_eq!(media_mime("pdf"), Some("application/pdf"));
        assert_eq!(media_mime("mp4"), Some("video/mp4"));
        assert_eq!(media_mime("webm"), Some("video/webm"));
        assert_eq!(media_mime("mp3"), Some("audio/mpeg"));
        assert_eq!(media_mime("flac"), Some("audio/flac"));
        // SVG 与未知类型不在此列（SVG 另有硬性拒绝）
        assert_eq!(media_mime("svg"), None);
        assert_eq!(media_mime("exe"), None);
    }

    #[test]
    fn pdf_and_media_unified_limit_over_text_limit() {
        // PDF 不属于图片也不属于文本分支：走可配上限（>1MB 也可预览）
        let ext = "pdf";
        assert!(media_mime(ext).is_some());
        assert!(!IMAGE_EXTS.contains(&ext));
    }
}
