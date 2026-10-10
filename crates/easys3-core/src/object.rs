//! 桶内对象辅助操作：详情、新建虚拟文件夹与历史 multipart 残片管理。
//! 规格：`.agents/s3-operations.md` §6、§7。

use crate::error::{classify, CoreError};
use aws_sdk_s3::{primitives::ByteStream, Client};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
pub struct ObjectDetail {
    pub key: String,
    pub size: u64,
    pub last_modified: Option<String>,
    pub e_tag: Option<String>,
    pub storage_class: Option<String>,
    pub content_type: Option<String>,
    pub metadata: Vec<MetadataEntry>,
    pub restore_status: Option<String>,
}

pub async fn restore_archived_object(
    client: &Client,
    bucket: &str,
    key: &str,
    days: i32,
    tier: &str,
) -> Result<(), CoreError> {
    if !(1..=365).contains(&days) {
        return Err(CoreError::Other(
            "恢复保留天数必须在 1 至 365 天之间".into(),
        ));
    }
    let tier = tier
        .parse::<aws_sdk_s3::types::Tier>()
        .map_err(|_| CoreError::Other("不支持的恢复速度".into()))?;
    let request = aws_sdk_s3::types::RestoreRequest::builder()
        .days(days)
        .glacier_job_parameters(
            aws_sdk_s3::types::GlacierJobParameters::builder()
                .tier(tier)
                .build()
                .map_err(|e| CoreError::Other(format!("恢复请求无效：{e}")))?,
        )
        .build();
    client
        .restore_object()
        .bucket(bucket)
        .key(key)
        .restore_request(request)
        .send()
        .await
        .map_err(|e| classify(&e))?;
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct MetadataEntry {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MultipartUploadInfo {
    pub key: String,
    pub upload_id: String,
    pub initiated: Option<String>,
    pub storage_class: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MultipartPartInfo {
    pub part_number: i32,
    pub size: u64,
    pub last_modified: Option<String>,
    pub e_tag: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ObjectVersionInfo {
    pub version_id: String,
    pub size: u64,
    pub last_modified: Option<String>,
    pub is_delete_marker: bool,
    pub is_latest: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectTag {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ObjectAclInfo {
    pub owner: Option<String>,
    pub grants: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BucketVersioningInfo {
    pub status: String,
}

pub async fn create_bucket(client: &Client, bucket: &str) -> Result<(), CoreError> {
    if !crate::config::is_valid_bucket_name(bucket) {
        return Err(CoreError::Other(
            "桶名不符合规范（3–63 位小写字母、数字、点或连字符）".into(),
        ));
    }
    client
        .create_bucket()
        .bucket(bucket)
        .send()
        .await
        .map_err(|e| classify(&e))?;
    Ok(())
}

pub async fn delete_bucket(client: &Client, bucket: &str) -> Result<(), CoreError> {
    client
        .delete_bucket()
        .bucket(bucket)
        .send()
        .await
        .map_err(|e| classify(&e))?;
    Ok(())
}

pub async fn bucket_versioning(
    client: &Client,
    bucket: &str,
) -> Result<BucketVersioningInfo, CoreError> {
    let out = client
        .get_bucket_versioning()
        .bucket(bucket)
        .send()
        .await
        .map_err(|e| classify(&e))?;
    Ok(BucketVersioningInfo {
        status: out
            .status()
            .map(|status| status.as_str().to_owned())
            .unwrap_or_else(|| "Suspended".into()),
    })
}

pub async fn set_bucket_versioning(
    client: &Client,
    bucket: &str,
    enabled: bool,
) -> Result<(), CoreError> {
    let status = if enabled {
        aws_sdk_s3::types::BucketVersioningStatus::Enabled
    } else {
        aws_sdk_s3::types::BucketVersioningStatus::Suspended
    };
    let configuration = aws_sdk_s3::types::VersioningConfiguration::builder()
        .status(status)
        .build();
    client
        .put_bucket_versioning()
        .bucket(bucket)
        .versioning_configuration(configuration)
        .send()
        .await
        .map_err(|e| classify(&e))?;
    Ok(())
}

pub async fn get_object_tags(
    client: &Client,
    bucket: &str,
    key: &str,
) -> Result<Vec<ObjectTag>, CoreError> {
    let out = client
        .get_object_tagging()
        .bucket(bucket)
        .key(key)
        .send()
        .await
        .map_err(|e| classify(&e))?;
    Ok(out
        .tag_set()
        .iter()
        .map(|tag| ObjectTag {
            key: tag.key().to_owned(),
            value: tag.value().to_owned(),
        })
        .collect())
}

pub async fn put_object_tags(
    client: &Client,
    bucket: &str,
    key: &str,
    tags: &[ObjectTag],
) -> Result<(), CoreError> {
    if tags.iter().any(|tag| tag.key.trim().is_empty()) {
        return Err(CoreError::Other("标签名称不能为空".into()));
    }
    let tag_set = aws_sdk_s3::types::Tagging::builder()
        .set_tag_set(Some(
            tags.iter()
                .map(|tag| {
                    aws_sdk_s3::types::Tag::builder()
                        .key(&tag.key)
                        .value(&tag.value)
                        .build()
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| CoreError::Other(format!("标签格式无效：{error}")))?,
        ))
        .build()
        .map_err(|error| CoreError::Other(format!("标签格式无效：{error}")))?;
    client
        .put_object_tagging()
        .bucket(bucket)
        .key(key)
        .tagging(tag_set)
        .send()
        .await
        .map_err(|e| classify(&e))?;
    Ok(())
}

pub async fn get_object_acl(
    client: &Client,
    bucket: &str,
    key: &str,
) -> Result<ObjectAclInfo, CoreError> {
    let out = client
        .get_object_acl()
        .bucket(bucket)
        .key(key)
        .send()
        .await
        .map_err(|e| classify(&e))?;
    let grants = out
        .grants()
        .iter()
        .map(|grant| {
            format!(
                "{}：{}",
                grant
                    .grantee()
                    .and_then(|g| g.display_name().or(g.uri()).or(g.id()))
                    .unwrap_or("未知主体"),
                grant.permission().map(|p| p.as_str()).unwrap_or("未知权限")
            )
        })
        .collect();
    Ok(ObjectAclInfo {
        owner: out
            .owner()
            .and_then(|owner| owner.display_name().or(owner.id()))
            .map(ToOwned::to_owned),
        grants,
    })
}

pub async fn list_object_versions(
    client: &Client,
    bucket: &str,
    key: &str,
) -> Result<Vec<ObjectVersionInfo>, CoreError> {
    let mut result = Vec::new();
    let mut key_marker = None;
    let mut version_id_marker = None;
    loop {
        let mut request = client.list_object_versions().bucket(bucket).prefix(key);
        if let Some(marker) = key_marker.as_deref() {
            request = request.key_marker(marker);
        }
        if let Some(marker) = version_id_marker.as_deref() {
            request = request.version_id_marker(marker);
        }
        let out = request.send().await.map_err(|error| {
            let classified = classify(&error);
            CoreError::Other(format!(
                "无法读取版本：桶可能未开启版本控制，或没有权限（{classified}）"
            ))
        })?;
        result.extend(
            out.versions()
                .iter()
                .filter(|version| version.key() == Some(key))
                .filter_map(|version| {
                    Some(ObjectVersionInfo {
                        version_id: version.version_id()?.to_owned(),
                        size: version.size().unwrap_or(0).max(0) as u64,
                        last_modified: version.last_modified().map(ToString::to_string),
                        is_delete_marker: false,
                        is_latest: version.is_latest().unwrap_or(false),
                    })
                }),
        );
        result.extend(
            out.delete_markers()
                .iter()
                .filter(|marker| marker.key() == Some(key))
                .filter_map(|marker| {
                    Some(ObjectVersionInfo {
                        version_id: marker.version_id()?.to_owned(),
                        size: 0,
                        last_modified: marker.last_modified().map(ToString::to_string),
                        is_delete_marker: true,
                        is_latest: marker.is_latest().unwrap_or(false),
                    })
                }),
        );
        if !out.is_truncated().unwrap_or(false) {
            break;
        }
        key_marker = out.next_key_marker().map(ToOwned::to_owned);
        version_id_marker = out.next_version_id_marker().map(ToOwned::to_owned);
    }
    result.sort_by(|a, b| b.last_modified.cmp(&a.last_modified));
    Ok(result)
}

/// 将指定历史版本复制为当前对象。删除标记不能恢复为对象内容。
pub async fn restore_object_version(
    client: &Client,
    bucket: &str,
    key: &str,
    version_id: &str,
) -> Result<(), CoreError> {
    let source = format!("{}?versionId={}", copy_source(bucket, key), version_id);
    client
        .copy_object()
        .bucket(bucket)
        .key(key)
        .copy_source(source)
        .send()
        .await
        .map_err(|e| classify(&e))?;
    Ok(())
}

pub async fn head_object(
    client: &Client,
    bucket: &str,
    key: &str,
) -> Result<ObjectDetail, CoreError> {
    let out = client
        .head_object()
        .bucket(bucket)
        .key(key)
        .send()
        .await
        .map_err(|e| classify(&e))?;
    let mut metadata: Vec<_> = out
        .metadata()
        .as_ref()
        .into_iter()
        .flat_map(|metadata| metadata.iter())
        .map(|(key, value)| MetadataEntry {
            key: key.clone(),
            value: value.clone(),
        })
        .collect();
    metadata.sort_by(|a, b| a.key.cmp(&b.key));
    Ok(ObjectDetail {
        key: key.to_owned(),
        size: out.content_length().unwrap_or_default().max(0) as u64,
        last_modified: out.last_modified().map(ToString::to_string),
        e_tag: out.e_tag().map(ToOwned::to_owned),
        storage_class: out.storage_class().map(|value| value.as_str().to_owned()),
        content_type: out.content_type().map(ToOwned::to_owned),
        metadata,
        restore_status: out.restore().map(ToOwned::to_owned),
    })
}

pub async fn create_folder(
    client: &Client,
    bucket: &str,
    prefix: &str,
    name: &str,
) -> Result<(), CoreError> {
    let name = name.trim();
    if name.is_empty() || name.contains('/') {
        return Err(CoreError::Other(
            "文件夹名称不能为空且不能包含 /".to_string(),
        ));
    }
    client
        .put_object()
        .bucket(bucket)
        .key(format!("{prefix}{name}/"))
        .body(ByteStream::from(Vec::new()))
        .send()
        .await
        .map_err(|e| classify(&e))?;
    Ok(())
}

pub async fn create_empty_folders(
    client: &Client,
    bucket: &str,
    keys: &[String],
) -> Result<(), CoreError> {
    for key in keys {
        client
            .put_object()
            .bucket(bucket)
            .key(key)
            .body(ByteStream::from(Vec::new()))
            .send()
            .await
            .map_err(|e| classify(&e))?;
    }
    Ok(())
}

/// S3 CopySource 要求 bucket/key 进行 URL 编码；`/` 用于保留对象路径分隔。
fn copy_source(bucket: &str, key: &str) -> String {
    let mut source = String::with_capacity(bucket.len() + key.len() + 1);
    source.push_str(bucket);
    source.push('/');
    for byte in key.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'_' | b'.' | b'~') {
            source.push(byte as char);
        } else {
            use std::fmt::Write;
            let _ = write!(source, "%{byte:02X}");
        }
    }
    source
}

/// S3 CopyObject 单请求上限：超过 5GB 必须分片复制，P0 不支持（规格 §6）
pub const COPY_MAX_BYTES: i64 = 5 * 1024 * 1024 * 1024;

/// 单对象复制结果
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyOutcome {
    /// 已复制（移动模式下源对象已删除）
    Copied,
    /// 冲突策略"跳过"且目标已存在，未执行
    Skipped,
}

/// 桶内单对象复制 / 移动（服务端 `copy_object`，不做整对象下载中转，规格 §6）。
/// - 大对象（> 5GB）明确报错，不静默失败（P0 不做分片复制）；
/// - `skip_if_exists`：目标已存在时跳过（统一冲突策略"跳过"）；
/// - `remove_source`：复制成功后删除源对象（移动 / 重命名语义）。
pub async fn copy_object(
    client: &Client,
    source_bucket: &str,
    target_bucket: &str,
    source_key: &str,
    target_key: &str,
    remove_source: bool,
    skip_if_exists: bool,
) -> Result<CopyOutcome, CoreError> {
    if source_key.is_empty() || target_key.is_empty() {
        return Err(CoreError::Other("源对象和目标对象不能为空".to_string()));
    }
    if source_key == target_key {
        return Err(CoreError::Other("目标名称不能与原对象相同".to_string()));
    }
    // 大小前置检查：>5GB 的 copy_object 会被服务端拒绝，提前给出明确提示
    let head = client
        .head_object()
        .bucket(source_bucket)
        .key(source_key)
        .send()
        .await
        .map_err(|e| classify(&e))?;
    if head.content_length().unwrap_or(0) > COPY_MAX_BYTES {
        return Err(CoreError::Other(
            "对象超过 5GB，暂不支持服务端分片复制，请下载后重新上传".to_string(),
        ));
    }
    if skip_if_exists {
        match client
            .head_object()
            .bucket(target_bucket)
            .key(target_key)
            .send()
            .await
        {
            Ok(_) => return Ok(CopyOutcome::Skipped),
            Err(e) => {
                let err = classify(&e);
                // 404 = 目标不存在，继续复制；其余错误如实上抛
                if !matches!(err.kind(), crate::error::ErrorKind::NotFound) {
                    return Err(err);
                }
            }
        }
    }
    client
        .copy_object()
        .bucket(target_bucket)
        .key(target_key)
        .copy_source(copy_source(source_bucket, source_key))
        .send()
        .await
        .map_err(|e| classify(&e))?;
    if remove_source {
        client
            .delete_object()
            .bucket(source_bucket)
            .key(source_key)
            .send()
            .await
            .map_err(|e| classify(&e))?;
    }
    Ok(CopyOutcome::Copied)
}

/// 以 CopyObject 原地替换 Content-Type 与自定义元数据；S3 单请求 Copy 上限 5GB。
pub async fn replace_object_metadata(
    client: &Client,
    bucket: &str,
    key: &str,
    content_type: &str,
    metadata: &[(String, String)],
) -> Result<(), CoreError> {
    let head = client
        .head_object()
        .bucket(bucket)
        .key(key)
        .send()
        .await
        .map_err(|e| classify(&e))?;
    if head.content_length().unwrap_or(0) > COPY_MAX_BYTES {
        return Err(CoreError::Other(
            "对象超过 5GB，暂不支持原地编辑元数据".into(),
        ));
    }
    if content_type.trim().is_empty() {
        return Err(CoreError::Other("Content-Type 不能为空".into()));
    }
    let mut map = std::collections::HashMap::new();
    for (name, value) in metadata {
        if name.trim().is_empty() {
            return Err(CoreError::Other("自定义元数据名称不能为空".into()));
        }
        map.insert(name.trim().to_string(), value.clone());
    }
    client
        .copy_object()
        .bucket(bucket)
        .key(key)
        .copy_source(copy_source(bucket, key))
        .metadata_directive(aws_sdk_s3::types::MetadataDirective::Replace)
        .content_type(content_type.trim())
        .set_metadata(Some(map))
        .send()
        .await
        .map_err(|e| classify(&e))?;
    Ok(())
}

pub async fn list_multipart_uploads(
    client: &Client,
    bucket: &str,
) -> Result<Vec<MultipartUploadInfo>, CoreError> {
    let mut result = Vec::new();
    let mut key_marker: Option<String> = None;
    let mut upload_id_marker: Option<String> = None;
    loop {
        let mut request = client.list_multipart_uploads().bucket(bucket);
        if let Some(marker) = key_marker.as_deref() {
            request = request.key_marker(marker);
        }
        if let Some(marker) = upload_id_marker.as_deref() {
            request = request.upload_id_marker(marker);
        }
        let out = request.send().await.map_err(|e| classify(&e))?;
        result.extend(out.uploads().iter().filter_map(|upload| {
            Some(MultipartUploadInfo {
                key: upload.key()?.to_owned(),
                upload_id: upload.upload_id()?.to_owned(),
                initiated: upload.initiated().map(ToString::to_string),
                storage_class: upload
                    .storage_class()
                    .map(|value| value.as_str().to_owned()),
            })
        }));
        if !out.is_truncated().unwrap_or(false) {
            break;
        }
        key_marker = out.next_key_marker().map(ToOwned::to_owned);
        upload_id_marker = out.next_upload_id_marker().map(ToOwned::to_owned);
        if key_marker.is_none() {
            break;
        }
    }
    Ok(result)
}

/// 批量中止未完成上传（规格 §7）。返回清理**失败**的条目：
/// 调用方据此刷新列表（成功项移除、失败项保留原信息可重试）。
pub async fn abort_multipart_uploads(
    client: &Client,
    bucket: &str,
    uploads: &[MultipartUploadInfo],
) -> Vec<MultipartUploadInfo> {
    let mut failed = Vec::new();
    for upload in uploads {
        let result = client
            .abort_multipart_upload()
            .bucket(bucket)
            .key(&upload.key)
            .upload_id(&upload.upload_id)
            .send()
            .await;
        if result.is_err() {
            failed.push(upload.clone());
        }
    }
    failed
}

pub async fn list_multipart_parts(
    client: &Client,
    bucket: &str,
    key: &str,
    upload_id: &str,
) -> Result<Vec<MultipartPartInfo>, CoreError> {
    let mut result = Vec::new();
    let mut marker: Option<String> = None;
    loop {
        let mut request = client
            .list_parts()
            .bucket(bucket)
            .key(key)
            .upload_id(upload_id);
        if let Some(value) = marker {
            request = request.part_number_marker(value);
        }
        let out = request.send().await.map_err(|e| classify(&e))?;
        result.extend(out.parts().iter().map(|part| MultipartPartInfo {
            part_number: part.part_number().unwrap_or_default(),
            size: part.size().unwrap_or_default().max(0) as u64,
            last_modified: part.last_modified().map(ToString::to_string),
            e_tag: part.e_tag().map(ToOwned::to_owned),
        }));
        if !out.is_truncated().unwrap_or(false) {
            break;
        }
        marker = out.next_part_number_marker().map(ToOwned::to_owned);
        if marker.is_none() {
            break;
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multipart_info_is_serializable_for_ipc() {
        let value = MultipartUploadInfo {
            key: "目录/文件.bin".into(),
            upload_id: "upload-id".into(),
            initiated: None,
            storage_class: Some("STANDARD".into()),
        };
        let json = serde_json::to_string(&value).unwrap();
        assert!(json.contains("目录/文件.bin"));
    }

    #[test]
    fn copy_source_keeps_path_and_encodes_unicode() {
        assert_eq!(
            copy_source("bucket", "目录/文件 name.txt"),
            "bucket/%E7%9B%AE%E5%BD%95/%E6%96%87%E4%BB%B6%20name.txt"
        );
    }
}
