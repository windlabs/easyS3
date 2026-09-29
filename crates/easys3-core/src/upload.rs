//! 上传：小文件单 PUT，大文件手动 multipart。
//! 规格：`.agents/s3-operations.md`「2. 上传」。
//!
//! 硬性约束：
//! - aws-sdk-s3 (Rust) 的 put_object 不会自动分片，大文件必须手动
//!   create_multipart_upload → upload_part → complete_multipart_upload；
//! - 分片 ≥ 5MB（S3 限制，最后一片除外），单文件最多 10000 片；
//! - 流式读取本地文件，禁止整文件读入内存（一次仅一个分片在内存）；
//! - 取消或失败必须 abort_multipart_upload 清理残片。

use crate::error::{classify, CoreError};
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::types::{CompletedMultipartUpload, CompletedPart};
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

/// 进度回调：(已传字节, 总字节)，针对单个文件
pub type ProgressFn = Arc<dyn Fn(u64, u64) + Send + Sync>;

/// 计算分片布局：分片大小与数量。
/// part_size 至少 PART_SIZE，且保证分片数不超过 MAX_PARTS（每片除最后一片外 ≥5MB 的 S3 约束由 8MB 起步保证）。
pub fn part_layout(size: u64) -> (u64, u64) {
    let part_size = PART_SIZE.max(size.div_ceil(MAX_PARTS));
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
) -> Result<(), CoreError> {
    let size = tokio::fs::metadata(path)
        .await
        .map_err(|e| CoreError::io(format!("读取文件失败 {}", path.display()), e))?
        .len();

    if size < MULTIPART_THRESHOLD {
        progress(0, size);
        // from_path 惰性流式读取；显式 content_length 避免 chunked 传输（部分兼容服务不支持）
        let body = ByteStream::from_path(path)
            .await
            .map_err(|e| CoreError::io(format!("打开文件失败 {}", path.display()), e))?;
        client
            .put_object()
            .bucket(bucket)
            .key(key)
            .body(body)
            .content_length(size as i64)
            .send()
            .await
            .map_err(|e| classify(&e))?;
        progress(size, size);
        Ok(())
    } else {
        upload_multipart(client, bucket, key, path, size, progress, cancel).await
    }
}

async fn upload_multipart(
    client: &Client,
    bucket: &str,
    key: &str,
    path: &Path,
    size: u64,
    progress: ProgressFn,
    cancel: Arc<AtomicBool>,
) -> Result<(), CoreError> {
    let mpu = client
        .create_multipart_upload()
        .bucket(bucket)
        .key(key)
        .send()
        .await
        .map_err(|e| classify(&e))?;
    let target = PartTarget {
        client,
        bucket,
        key,
        upload_id: mpu.upload_id().unwrap_or_default(),
    };

    let result = upload_parts(&target, path, size, &progress, &cancel).await;

    match result {
        Ok(()) => Ok(()),
        Err(e) => {
            // 取消/失败都要 abort，否则残片永久占用存储（规格）
            let _ = client
                .abort_multipart_upload()
                .bucket(bucket)
                .key(key)
                .upload_id(target.upload_id)
                .send()
                .await;
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

async fn upload_parts(
    target: &PartTarget<'_>,
    path: &Path,
    size: u64,
    progress: &ProgressFn,
    cancel: &Arc<AtomicBool>,
) -> Result<(), CoreError> {
    let PartTarget {
        client,
        bucket,
        key,
        upload_id,
    } = *target;
    let (part_size, part_count) = part_layout(size);
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|e| CoreError::io(format!("打开文件失败 {}", path.display()), e))?;

    let mut completed: Vec<CompletedPart> = Vec::new();
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
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn part_layout_small_file_uses_default() {
        let (ps, n) = part_layout(100 * 1024 * 1024);
        assert_eq!(ps, PART_SIZE);
        assert_eq!(n, 13); // 100MB / 8MB = 12.5 → 13 片
    }

    #[test]
    fn part_layout_respects_max_parts() {
        // 100GB：100 * 2^30 / 8MB = 12800 片 > 10000 → 分片加大
        let size = 100u64 * 1024 * 1024 * 1024;
        let (ps, n) = part_layout(size);
        assert!(n <= MAX_PARTS, "分片数 {n} 必须不超过 {MAX_PARTS}");
        assert!(ps >= 5 * 1024 * 1024, "分片必须 ≥5MB");
        assert!(ps * n >= size, "分片总容量必须覆盖文件大小");
    }

    #[test]
    fn part_layout_exact_multiple() {
        let (ps, n) = part_layout(PART_SIZE * 3);
        assert_eq!((ps, n), (PART_SIZE, 3));
    }
}
