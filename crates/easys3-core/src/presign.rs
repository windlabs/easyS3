//! P1-02 预签名下载 / 上传 URL。

use crate::error::CoreError;
use aws_sdk_s3::{presigning::PresigningConfig, Client};
use std::time::Duration;

pub const MAX_EXPIRY_SECONDS: u64 = 7 * 24 * 60 * 60;

fn config(seconds: u64) -> Result<PresigningConfig, CoreError> {
    if seconds == 0 || seconds > MAX_EXPIRY_SECONDS {
        return Err(CoreError::Other("有效期必须在 1 秒至 7 天之间".into()));
    }
    PresigningConfig::expires_in(Duration::from_secs(seconds))
        .map_err(|error| CoreError::Other(format!("生成签名配置失败：{error}")))
}

pub async fn get_url(
    client: &Client,
    bucket: &str,
    key: &str,
    seconds: u64,
) -> Result<String, CoreError> {
    client
        .get_object()
        .bucket(bucket)
        .key(key)
        .presigned(config(seconds)?)
        .await
        .map(|request| request.uri().to_string())
        .map_err(|error| CoreError::Other(format!("生成下载链接失败：{error}")))
}

pub async fn put_url(
    client: &Client,
    bucket: &str,
    key: &str,
    seconds: u64,
) -> Result<String, CoreError> {
    client
        .put_object()
        .bucket(bucket)
        .key(key)
        .presigned(config(seconds)?)
        .await
        .map(|request| request.uri().to_string())
        .map_err(|error| CoreError::Other(format!("生成上传链接失败：{error}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn expiry_bounds() {
        assert!(config(0).is_err());
        assert!(config(MAX_EXPIRY_SECONDS + 1).is_err());
        assert!(config(300).is_ok());
    }
}
