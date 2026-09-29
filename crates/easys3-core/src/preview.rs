//! 对象预览：图片与 UTF-8 文本；SVG 一律不预览（webview XSS 风险）。
//! 规格：`.agents/s3-operations.md`「5. 对象预览」。

use crate::error::{classify, CoreError};
use aws_sdk_s3::Client;
use base64::Engine;
use serde::Serialize;
use tokio::io::AsyncReadExt;

pub const TEXT_MAX_BYTES: i64 = 1024 * 1024;
pub const IMAGE_MAX_BYTES: i64 = 20 * 1024 * 1024;

const IMAGE_EXTS: &[&str] = &["jpg", "jpeg", "png", "gif", "webp", "bmp"];

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PreviewData {
    Text { content: String },
    Image { mime: String, data_base64: String },
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

/// 预览对象。超限 / 二进制 / SVG 返回可读错误提示改用下载。
pub async fn preview_object(
    client: &Client,
    bucket: &str,
    key: &str,
) -> Result<PreviewData, CoreError> {
    let ext = ext_of(key);
    if ext == "svg" {
        return Err(CoreError::Other(
            "SVG 文件不支持预览，请下载后查看".to_string(),
        ));
    }

    // 先 Head 拿大小，超限直接拒绝，避免无谓下载
    let head = client
        .head_object()
        .bucket(bucket)
        .key(key)
        .send()
        .await
        .map_err(|e| classify(&e))?;
    let len = head.content_length().unwrap_or(0);

    let is_image = IMAGE_EXTS.contains(&ext.as_str());
    if is_image && len > IMAGE_MAX_BYTES {
        return Err(CoreError::Other("图片超过 20MB，请下载后查看".to_string()));
    }
    if !is_image && len > TEXT_MAX_BYTES {
        return Err(CoreError::Other("文本超过 1MB，请下载后查看".to_string()));
    }

    let out = client
        .get_object()
        .bucket(bucket)
        .key(key)
        .send()
        .await
        .map_err(|e| classify(&e))?;
    let mut bytes = Vec::new();
    out.body
        .into_async_read()
        .read_to_end(&mut bytes)
        .await
        .map_err(|e| CoreError::io("读取对象失败", e))?;

    if is_image {
        Ok(PreviewData::Image {
            mime: image_mime(&ext).to_string(),
            data_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
        })
    } else {
        match String::from_utf8(bytes) {
            Ok(text) => Ok(PreviewData::Text { content: text }),
            Err(_) => Err(CoreError::Other("二进制文件，请下载后查看".to_string())),
        }
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
}
