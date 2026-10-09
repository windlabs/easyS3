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
    bucket: &str,
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
        .bucket(bucket)
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
            .bucket(bucket)
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
        .bucket(bucket)
        .key(target_key)
        .copy_source(copy_source(bucket, source_key))
        .send()
        .await
        .map_err(|e| classify(&e))?;
    if remove_source {
        client
            .delete_object()
            .bucket(bucket)
            .key(source_key)
            .send()
            .await
            .map_err(|e| classify(&e))?;
    }
    Ok(CopyOutcome::Copied)
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
