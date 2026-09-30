//! S3 客户端构建与连通性测试。

use crate::config::ConnectionConfig;
use crate::error::{classify, CoreError};
use aws_sdk_s3::config::{BehaviorVersion, Credentials, Region};
use aws_sdk_s3::Client;

/// 按连接配置构建 S3 客户端。
/// 自定义 endpoint + force_path_style 支持非 AWS 的 S3 兼容服务（规格硬性要求）。
pub fn build_client(p: &ConnectionConfig) -> Result<Client, CoreError> {
    let creds = Credentials::new(
        p.access_key.trim(),
        p.secret_key.trim(),
        None,
        None,
        "easys3-static",
    );
    let cfg = aws_sdk_s3::Config::builder()
        .endpoint_url(p.endpoint_url.trim())
        .region(Region::new(p.region.trim().to_string()))
        .credentials_provider(creds)
        .force_path_style(p.force_path_style)
        .behavior_version(BehaviorVersion::latest())
        .build();
    Ok(Client::from_conf(cfg))
}

/// 测试连接：无 default_bucket 时 ListBuckets；有则对该桶 HeadBucket（规格）。
pub async fn test_connection(p: &ConnectionConfig) -> Result<(), CoreError> {
    let client = build_client(p)?;
    match p.default_bucket.as_deref() {
        Some(bucket) => {
            client
                .head_bucket()
                .bucket(bucket)
                .send()
                .await
                .map_err(|e| classify(&e))?;
        }
        None => {
            client
                .list_buckets()
                .send()
                .await
                .map_err(|e| classify(&e))?;
        }
    }
    Ok(())
}
