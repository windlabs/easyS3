//! 上传：小文件单 PUT，大文件手动 multipart。
//! 规格：`.agents/s3-operations.md`「2. 上传」。
//!
//! 硬性约束：
//! - aws-sdk-s3 (Rust) 的 put_object 不会自动分片，大文件必须手动
//!   create_multipart_upload → upload_part → complete_multipart_upload；
//! - 分片 ≥ 5MB（S3 限制，最后一片除外），单文件最多 10000 片；
//! - 流式读取本地文件，禁止整文件读入内存（一次仅一个分片在内存）；
//! - 取消或失败必须 abort_multipart_upload 清理残片。

use crate::client_crypto::{encrypt_file, ClientEncryptionKey};
use crate::error::{classify, CoreError};
use crate::throttle::Throttle;
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::types::{CompletedMultipartUpload, CompletedPart, StorageClass};
use aws_sdk_s3::Client;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::io::AsyncReadExt;

/// 超过该大小走 multipart
pub const MULTIPART_THRESHOLD: u64 = 16 * 1024 * 1024;
/// 分片大小建议 8–16MB（规格），取 8MB
pub const PART_SIZE: u64 = 8 * 1024 * 1024;
/// S3 单文件分片数上限
pub const MAX_PARTS: u64 = 10_000;
/// S3 分片下限（除最后一片外 ≥5MB）
pub const MIN_PART_SIZE: u64 = 5 * 1024 * 1024;

/// 进度回调：(已传字节, 总字节)，针对单个文件
pub type ProgressFn = Arc<dyn Fn(u64, u64) + Send + Sync>;

/// 单次上传的可调参数（设置页可配，规格 §2）
#[derive(Clone, Default)]
pub struct UploadOptions {
    /// 目标存储类别；None = STANDARD
    pub storage_class: Option<StorageClass>,
    /// multipart 分片大小（字节）；0 = 默认 8MB。低于 5MB 由设置层收敛。
    pub part_size: u64,
    /// 全局上传限速；None = 不限速
    pub throttle: Option<Arc<Throttle>>,
    /// 上传完成后 HeadObject 回读 ETag；multipart 复合 ETag 不做 MD5 比较。
    pub verify_after_upload: bool,
    pub encryption: Option<ServerEncryption>,
    /// 客户端 AES-256-GCM；密钥只由调用方在内存中传入。
    pub client_encryption: Option<ClientEncryptionKey>,
    /// 内部字段：加密对象记录原始长度；不由前端或持久化任务提供。
    pub client_plain_size: Option<u64>,
}

#[derive(Clone, Debug)]
pub enum ServerEncryption {
    S3,
    Kms {
        key_id: Option<String>,
    },
    Customer {
        key_base64: String,
        key_md5_base64: String,
    },
}

/// multipart 上传的可持久化检查点。调用方负责将它写入应用配置目录。
#[derive(Clone, Debug, Default)]
pub struct MultipartCheckpoint {
    pub upload_id: Option<String>,
    pub part_size: u64,
    pub completed_parts: Vec<(i32, String)>,
}

pub type CheckpointFn = Arc<dyn Fn(MultipartCheckpoint) + Send + Sync>;

/// 计算分片布局：分片大小与数量。
/// part_size_cfg = 0 用默认值；非 0 时强制不低于 5MB（S3 硬性约束，规格 §2），
/// 且始终保证分片数不超过 MAX_PARTS。
pub fn part_layout(size: u64, part_size_cfg: u64) -> (u64, u64) {
    let base = if part_size_cfg == 0 {
        PART_SIZE
    } else {
        part_size_cfg.max(MIN_PART_SIZE)
    };
    let part_size = base.max(size.div_ceil(MAX_PARTS));
    let count = size.div_ceil(part_size);
    (part_size, count)
}

/// 上传单个文件。cancel 置位后中止并清理服务端残片。
pub async fn upload_file(
    client: &Client,
    bucket: &str,
    key: &str,
    path: &Path,
    progress: ProgressFn,
    cancel: Arc<AtomicBool>,
    opts: UploadOptions,
) -> Result<(), CoreError> {
    upload_file_resumable(
        client, bucket, key, path, progress, cancel, opts, None, None,
    )
    .await
}

/// 可恢复上传。已有 `UploadId` 时以服务端 ListParts 为事实来源，已完成分片不会重复上传。
/// 主动取消会 Abort；其它失败保留检查点，调用方可在下次启动恢复。
#[allow(clippy::too_many_arguments)]
pub async fn upload_file_resumable(
    client: &Client,
    bucket: &str,
    key: &str,
    path: &Path,
    progress: ProgressFn,
    cancel: Arc<AtomicBool>,
    opts: UploadOptions,
    checkpoint: Option<MultipartCheckpoint>,
    checkpoint_changed: Option<CheckpointFn>,
) -> Result<(), CoreError> {
    if let Some(encryption_key) = opts.client_encryption.clone() {
        // 加密后密文字节、分片边界均变化，恢复记录不能混用原文件检查点。
        let temporary = std::env::temp_dir().join(format!(
            "easys3-client-crypto-{:016x}",
            rand::random::<u64>()
        ));
        let plain_size = tokio::fs::metadata(path)
            .await
            .map_err(|e| CoreError::io(format!("读取文件失败 {}", path.display()), e))?
            .len();
        encrypt_file(path, &temporary, &encryption_key).await?;
        let mut encrypted_opts = opts;
        encrypted_opts.client_encryption = None;
        encrypted_opts.client_plain_size = Some(plain_size);
        let result = upload_file_resumable_inner(
            client,
            bucket,
            key,
            &temporary,
            progress,
            cancel,
            encrypted_opts,
            None,
            None,
        )
        .await;
        let _ = tokio::fs::remove_file(&temporary).await;
        return result;
    }
    upload_file_resumable_inner(
        client,
        bucket,
        key,
        path,
        progress,
        cancel,
        opts,
        checkpoint,
        checkpoint_changed,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn upload_file_resumable_inner(
    client: &Client,
    bucket: &str,
    key: &str,
    path: &Path,
    progress: ProgressFn,
    cancel: Arc<AtomicBool>,
    opts: UploadOptions,
    checkpoint: Option<MultipartCheckpoint>,
    checkpoint_changed: Option<CheckpointFn>,
) -> Result<(), CoreError> {
    let size = tokio::fs::metadata(path)
        .await
        .map_err(|e| CoreError::io(format!("读取文件失败 {}", path.display()), e))?
        .len();

    if size < MULTIPART_THRESHOLD {
        progress(0, size);
        if let Some(throttle) = opts.throttle.as_ref() {
            throttle.acquire(size).await;
            if cancel.load(Ordering::Relaxed) {
                return Err(CoreError::Cancelled);
            }
        }
        // from_path 惰性流式读取；显式 content_length 避免 chunked 传输（部分兼容服务不支持）
        let body = ByteStream::from_path(path)
            .await
            .map_err(|e| CoreError::io(format!("打开文件失败 {}", path.display()), e))?;
        let mut request = client
            .put_object()
            .bucket(bucket)
            .key(key)
            .body(body)
            .content_length(size as i64);
        if let Some(class) = opts.storage_class {
            request = request.storage_class(class);
        }
        if let Some(plain_size) = opts.client_plain_size {
            request = request
                .metadata("easys3-crypto", "aes256-gcm-v1")
                .metadata("easys3-crypto-plain-length", plain_size.to_string());
        }
        if let Some(encryption) = opts.encryption.as_ref() {
            request = match encryption {
                ServerEncryption::S3 => {
                    request.server_side_encryption(aws_sdk_s3::types::ServerSideEncryption::Aes256)
                }
                ServerEncryption::Kms { key_id } => {
                    let request = request
                        .server_side_encryption(aws_sdk_s3::types::ServerSideEncryption::AwsKms);
                    match key_id.as_deref() {
                        Some(key_id) => request.ssekms_key_id(key_id),
                        None => request,
                    }
                }
                ServerEncryption::Customer {
                    key_base64,
                    key_md5_base64,
                } => request
                    .sse_customer_algorithm("AES256")
                    .sse_customer_key(key_base64)
                    .sse_customer_key_md5(key_md5_base64),
            };
        }
        request.send().await.map_err(|e| classify(&e))?;
        if opts.verify_after_upload && opts.client_plain_size.is_none() {
            verify_uploaded_etag(client, bucket, key, path, size).await?;
        }
        progress(size, size);
        Ok(())
    } else {
        upload_multipart(
            client,
            bucket,
            key,
            path,
            size,
            progress,
            cancel,
            opts,
            checkpoint,
            checkpoint_changed,
        )
        .await
    }
}

async fn verify_uploaded_etag(
    client: &Client,
    bucket: &str,
    key: &str,
    path: &Path,
    size: u64,
) -> Result<(), CoreError> {
    let head = client
        .head_object()
        .bucket(bucket)
        .key(key)
        .send()
        .await
        .map_err(|e| classify(&e))?;
    let Some(etag) = head.e_tag().map(|value| value.trim_matches('"')) else {
        return Ok(());
    };
    if etag.contains('-') || etag.len() != 32 {
        return Ok(());
    }
    use md5::{Digest, Md5};
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|e| CoreError::io(format!("打开校验文件失败 {}", path.display()), e))?;
    let mut hasher = Md5::new();
    let mut buffer = [0u8; 256 * 1024];
    let mut read_total = 0;
    loop {
        let read = file
            .read(&mut buffer)
            .await
            .map_err(|e| CoreError::io("读取校验文件失败", e))?;
        if read == 0 {
            break;
        }
        read_total += read as u64;
        hasher.update(&buffer[..read]);
    }
    let actual = format!("{:x}", hasher.finalize());
    if read_total != size || actual != etag.to_ascii_lowercase() {
        return Err(CoreError::Other("上传后完整性校验失败：ETag 不一致".into()));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn upload_multipart(
    client: &Client,
    bucket: &str,
    key: &str,
    path: &Path,
    size: u64,
    progress: ProgressFn,
    cancel: Arc<AtomicBool>,
    opts: UploadOptions,
    checkpoint: Option<MultipartCheckpoint>,
    checkpoint_changed: Option<CheckpointFn>,
) -> Result<(), CoreError> {
    let old = checkpoint.unwrap_or_default();
    let (upload_id, completed_parts) = match old.upload_id.filter(|id| !id.is_empty()) {
        Some(upload_id) => {
            let response = client
                .list_parts()
                .bucket(bucket)
                .key(key)
                .upload_id(&upload_id)
                .send()
                .await
                .map_err(|e| classify(&e))?;
            let parts = response
                .parts()
                .iter()
                .filter_map(|part| Some((part.part_number()?, part.e_tag()?.to_owned())))
                .collect();
            (upload_id, parts)
        }
        None => {
            let mut request = client.create_multipart_upload().bucket(bucket).key(key);
            if let Some(ref class) = opts.storage_class {
                request = request.storage_class(class.clone());
            }
            if let Some(plain_size) = opts.client_plain_size {
                request = request
                    .metadata("easys3-crypto", "aes256-gcm-v1")
                    .metadata("easys3-crypto-plain-length", plain_size.to_string());
            }
            if let Some(encryption) = opts.encryption.as_ref() {
                request = match encryption {
                    ServerEncryption::S3 => request
                        .server_side_encryption(aws_sdk_s3::types::ServerSideEncryption::Aes256),
                    ServerEncryption::Kms { key_id } => {
                        let request = request.server_side_encryption(
                            aws_sdk_s3::types::ServerSideEncryption::AwsKms,
                        );
                        match key_id.as_deref() {
                            Some(key_id) => request.ssekms_key_id(key_id),
                            None => request,
                        }
                    }
                    ServerEncryption::Customer {
                        key_base64,
                        key_md5_base64,
                    } => request
                        .sse_customer_algorithm("AES256")
                        .sse_customer_key(key_base64)
                        .sse_customer_key_md5(key_md5_base64),
                };
            }
            let response = request.send().await.map_err(|e| classify(&e))?;
            (
                response.upload_id().unwrap_or_default().to_owned(),
                Vec::new(),
            )
        }
    };
    let part_size = if old.part_size > 0 {
        old.part_size
    } else {
        part_layout(size, opts.part_size).0
    };
    if let Some(callback) = checkpoint_changed.as_ref() {
        callback(MultipartCheckpoint {
            upload_id: Some(upload_id.clone()),
            part_size,
            completed_parts: completed_parts.clone(),
        });
    }
    let target = PartTarget {
        client,
        bucket,
        key,
        upload_id: &upload_id,
    };

    let result = upload_parts(
        &target,
        path,
        size,
        &progress,
        &cancel,
        &opts,
        part_size,
        completed_parts,
        checkpoint_changed.clone(),
    )
    .await;

    match result {
        Ok(()) => Ok(()),
        Err(e) => {
            // 旧调用路径没有检查点，仍按既有规则：失败/取消均 abort。
            // 只有启用检查点的恢复任务才会在异常失败后保留残片。
            if matches!(e, CoreError::Cancelled) || checkpoint_changed.is_none() {
                let _ = client
                    .abort_multipart_upload()
                    .bucket(bucket)
                    .key(key)
                    .upload_id(target.upload_id)
                    .send()
                    .await;
                if let Some(callback) = checkpoint_changed.as_ref() {
                    callback(MultipartCheckpoint::default());
                }
            }
            Err(e)
        }
    }
}

/// 分片上传目标（收敛参数，便于传递）
struct PartTarget<'a> {
    client: &'a Client,
    bucket: &'a str,
    key: &'a str,
    upload_id: &'a str,
}

#[allow(clippy::too_many_arguments)]
async fn upload_parts(
    target: &PartTarget<'_>,
    path: &Path,
    size: u64,
    progress: &ProgressFn,
    cancel: &Arc<AtomicBool>,
    opts: &UploadOptions,
    part_size: u64,
    prior_parts: Vec<(i32, String)>,
    checkpoint_changed: Option<CheckpointFn>,
) -> Result<(), CoreError> {
    let PartTarget {
        client,
        bucket,
        key,
        upload_id,
    } = *target;
    let (_, part_count) = part_layout(size, part_size);
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|e| CoreError::io(format!("打开文件失败 {}", path.display()), e))?;

    let mut completed: Vec<CompletedPart> = prior_parts
        .iter()
        .map(|(number, tag)| {
            CompletedPart::builder()
                .part_number(*number)
                .e_tag(tag)
                .build()
        })
        .collect();
    let completed_numbers: std::collections::HashSet<i32> =
        prior_parts.iter().map(|(number, _)| *number).collect();
    let mut done: u64 = 0;

    for part_number in 1..=(part_count as i32) {
        if cancel.load(Ordering::Relaxed) {
            return Err(CoreError::Cancelled);
        }
        let this_len = part_size.min(size - done) as usize;
        let mut buf = vec![0u8; this_len];
        file.read_exact(&mut buf)
            .await
            .map_err(|e| CoreError::io(format!("读取文件失败 {}", path.display()), e))?;

        if completed_numbers.contains(&part_number) {
            done += this_len as u64;
            progress(done, size);
            continue;
        }

        if let Some(throttle) = opts.throttle.as_ref() {
            throttle.acquire(this_len as u64).await;
            if cancel.load(Ordering::Relaxed) {
                return Err(CoreError::Cancelled);
            }
        }

        let up = client
            .upload_part()
            .bucket(bucket)
            .key(key)
            .upload_id(upload_id)
            .part_number(part_number)
            .body(ByteStream::from(buf))
            .send()
            .await
            .map_err(|e| classify(&e))?;

        completed.push(
            CompletedPart::builder()
                .e_tag(up.e_tag().unwrap_or_default())
                .part_number(part_number)
                .build(),
        );
        done += this_len as u64;
        progress(done, size);
        if let Some(callback) = checkpoint_changed.as_ref() {
            let parts = completed
                .iter()
                .filter_map(|part| Some((part.part_number()?, part.e_tag()?.to_owned())))
                .collect();
            callback(MultipartCheckpoint {
                upload_id: Some(upload_id.to_owned()),
                part_size,
                completed_parts: parts,
            });
        }
    }

    let parts = CompletedMultipartUpload::builder()
        .set_parts(Some(completed))
        .build();
    client
        .complete_multipart_upload()
        .bucket(bucket)
        .key(key)
        .upload_id(upload_id)
        .multipart_upload(parts)
        .send()
        .await
        .map_err(|e| classify(&e))?;
    if let Some(callback) = checkpoint_changed {
        callback(MultipartCheckpoint::default());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn part_layout_small_file_uses_default() {
        let (ps, n) = part_layout(100 * 1024 * 1024, 0);
        assert_eq!(ps, PART_SIZE);
        assert_eq!(n, 13); // 100MB / 8MB = 12.5 → 13 片
    }

    #[test]
    fn part_layout_respects_max_parts() {
        // 100GB：100 * 2^30 / 8MB = 12800 片 > 10000 → 分片加大
        let size = 100u64 * 1024 * 1024 * 1024;
        let (ps, n) = part_layout(size, 0);
        assert!(n <= MAX_PARTS, "分片数 {n} 必须不超过 {MAX_PARTS}");
        assert!(ps >= 5 * 1024 * 1024, "分片必须 ≥5MB");
        assert!(ps * n >= size, "分片总容量必须覆盖文件大小");
    }

    #[test]
    fn part_layout_exact_multiple() {
        let (ps, n) = part_layout(PART_SIZE * 3, 0);
        assert_eq!((ps, n), (PART_SIZE, 3));
    }

    #[test]
    fn part_layout_honors_configured_part_size() {
        // 设置 16MB 分片：新任务按新值分片；仍保证 ≥5MB 与 ≤10000 片
        let size = 100 * 1024 * 1024;
        let (ps, n) = part_layout(size, 16 * 1024 * 1024);
        assert_eq!(ps, 16 * 1024 * 1024);
        assert_eq!(n, 7); // 100MB / 16MB = 6.25 → 7 片
                          // 非法小值兜底：不低于 S3 的 5MB 分片下限
        let (ps, n) = part_layout(size, 1);
        assert_eq!(ps, MIN_PART_SIZE);
        assert!(n <= MAX_PARTS);
    }
}
