//! S3 客户端构建与连通性测试。

use crate::config::ConnectionConfig;
use crate::error::{classify, CoreError};
use aws_sdk_s3::config::{BehaviorVersion, Credentials, Region};
use aws_sdk_s3::Client;
use aws_smithy_runtime_api::client::http::SharedHttpClient;

/// 按连接配置构建 S3 客户端。
/// 自定义 endpoint + force_path_style 支持非 AWS 的 S3 兼容服务（规格硬性要求）。
/// 配置了 `ca_cert_path` 时构建信任该 CA 的 HTTP 客户端（自签证书场景，规格）。
pub fn build_client(p: &ConnectionConfig) -> Result<Client, CoreError> {
    let creds = Credentials::new(
        p.access_key.trim(),
        p.secret_key.trim(),
        None,
        None,
        "easys3-static",
    );
    let mut builder = aws_sdk_s3::Config::builder()
        .endpoint_url(p.endpoint_url.trim())
        .region(Region::new(p.region.trim().to_string()))
        .credentials_provider(creds)
        .force_path_style(p.force_path_style)
        .behavior_version(BehaviorVersion::latest());
    if let Some(path) = p
        .ca_cert_path
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        builder = builder.http_client(build_ca_http_client(path)?);
    }
    Ok(Client::from_conf(builder.build()))
}

/// 构建信任自定义 CA（PEM 文件）的 HTTP 客户端。
/// 走 aws-smithy-http-client 官方 TlsContext 路径：系统根证书与用户 CA 共同生效
/// （服务端证书由任一信任即可）；证书文件读取/解析失败给出中文错误（规格）。
fn build_ca_http_client(pem_path: &str) -> Result<SharedHttpClient, CoreError> {
    let pem = std::fs::read(pem_path)
        .map_err(|e| CoreError::io(format!("读取 CA 证书失败 {pem_path}"), e))?;
    let trust_store = aws_smithy_http_client::tls::TrustStore::default().with_pem_certificate(pem);
    let tls_context = aws_smithy_http_client::tls::TlsContext::builder()
        .with_trust_store(trust_store)
        .build()
        .map_err(|e| CoreError::Other(format!("CA 证书解析失败（{pem_path}）：{e}")))?;
    Ok(aws_smithy_http_client::Builder::new()
        .tls_provider(aws_smithy_http_client::tls::Provider::Rustls(
            aws_smithy_http_client::tls::rustls_provider::CryptoMode::AwsLc,
        ))
        .tls_context(tls_context)
        .build_https())
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
