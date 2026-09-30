//! S3 客户端构建与连通性测试。

use crate::config::ConnectionConfig;
use crate::error::{classify, CoreError};
use aws_sdk_s3::config::{BehaviorVersion, Credentials, Region, StalledStreamProtectionConfig};
use aws_sdk_s3::Client;
use aws_smithy_runtime_api::client::http::SharedHttpClient;
use std::time::Duration;

/// 控制面请求单次尝试超时（规格：浏览 / 测试 / 计数 / 预览 / 删除等小请求）
pub const CONTROL_ATTEMPT_TIMEOUT: Duration = Duration::from_secs(30);

/// 传输客户端：上传 / 下载对象体。**不设操作超时**——大文件在慢链路上合法地耗时很长；
/// 停滞流保护兜底：流 20 秒无任何数据即中断报错，死链不会挂死任务。
/// 自定义 endpoint + force_path_style 支持非 AWS 的 S3 兼容服务（规格硬性要求）。
/// 配置了 `ca_cert_path` 时构建信任该 CA 的 HTTP 客户端（自签证书场景，规格）。
pub fn build_client(p: &ConnectionConfig) -> Result<Client, CoreError> {
    build_configured_client(p, None)
}

/// 控制面客户端：列表 / Head / 计数 / 预览 / DeleteObjects 等小请求。
/// 单次尝试 30s 超时——服务端接受连接后不响应时，界面与任务不会无限等待（规格）。
pub fn build_control_client(p: &ConnectionConfig) -> Result<Client, CoreError> {
    build_configured_client(p, Some(CONTROL_ATTEMPT_TIMEOUT))
}

fn build_configured_client(
    p: &ConnectionConfig,
    attempt_timeout: Option<Duration>,
) -> Result<Client, CoreError> {
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
        .behavior_version(BehaviorVersion::latest())
        // 停滞流保护（默认 20s 宽限）：上传 / 下载流彻底无数据即中断，防止死链挂死
        .stalled_stream_protection(StalledStreamProtectionConfig::enabled().build());
    if let Some(timeout) = attempt_timeout {
        let timeouts = aws_smithy_types::timeout::TimeoutConfig::builder()
            .operation_attempt_timeout(timeout)
            .build();
        builder = builder.timeout_config(timeouts);
    }
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
/// 控制面客户端（30s 尝试超时）——测试不应无限等待。
pub async fn test_connection(p: &ConnectionConfig) -> Result<(), CoreError> {
    let client = build_control_client(p)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ConnectionConfig;

    fn conn() -> ConnectionConfig {
        ConnectionConfig {
            id: "t1".into(),
            name: "测试".into(),
            endpoint_url: "http://127.0.0.1:9000".into(),
            region: "us-east-1".into(),
            access_key: "ak".into(),
            secret_key: "sk".into(),
            force_path_style: true,
            default_bucket: None,
            ca_cert_path: None,
            last_test: None,
        }
    }

    /// 控制面客户端带 30s 单次尝试超时；传输客户端不设（大文件合法地慢，规格）
    #[test]
    fn client_timeout_policy() {
        let control = build_control_client(&conn()).unwrap();
        assert_eq!(
            control
                .config()
                .timeout_config()
                .and_then(|t| t.operation_attempt_timeout()),
            Some(CONTROL_ATTEMPT_TIMEOUT)
        );
        let transfer = build_client(&conn()).unwrap();
        assert_eq!(
            transfer
                .config()
                .timeout_config()
                .and_then(|t| t.operation_attempt_timeout()),
            None
        );
    }

    /// 两类客户端都启用停滞流保护（默认 20s 宽限：流彻底无数据即中断，防死链挂死）
    #[test]
    fn stalled_stream_protection_enabled() {
        let c = build_client(&conn()).unwrap();
        let ssp = c.config().stalled_stream_protection().unwrap();
        assert!(ssp.download_enabled());
        assert!(ssp.upload_enabled());
        assert_eq!(ssp.grace_period(), Duration::from_secs(20));
    }
}
