//! 下载：流式写盘 + 进度事件；取消时删除未完成的分段文件。
//! 规格：`.agents/s3-operations.md`「3. 下载」。

use crate::error::{classify, CoreError};
use crate::throttle::Throttle;
use aws_sdk_s3::Client;
use md5::{Digest, Md5};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::upload::ProgressFn;

/// 下载单个对象到 dest。自动创建父目录（文件夹下载保留目录结构）。
/// throttle = 全局下载限速（None / rate 0 = 不限速）。
#[allow(clippy::too_many_arguments)]
pub async fn download_object(
    client: &Client,
    bucket: &str,
    key: &str,
    dest: &Path,
    progress: ProgressFn,
    cancel: Arc<AtomicBool>,
    throttle: Option<Arc<Throttle>>,
) -> Result<(), CoreError> {
    let result =
        download_object_resumable(client, bucket, key, dest, progress, cancel, throttle, false)
            .await;
    if result.is_err() {
        let _ = tokio::fs::remove_file(partial_path(dest)).await;
    }
    result
}

/// 下载后 ETag 校验：仅单部件 ETag（32 位 MD5）可靠；multipart 复合 ETag 明确跳过。
pub async fn verify_download_etag(
    client: &Client,
    bucket: &str,
    key: &str,
    dest: &Path,
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
    let mut file = tokio::fs::File::open(dest)
        .await
        .map_err(|e| CoreError::io(format!("打开校验文件失败 {}", dest.display()), e))?;
    let mut hasher = Md5::new();
    let mut buffer = [0u8; 256 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .await
            .map_err(|e| CoreError::io("读取校验文件失败", e))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let actual = format!("{:x}", hasher.finalize());
    if actual != etag.to_ascii_lowercase() {
        return Err(CoreError::Other(format!(
            "完整性校验失败：ETag 不一致（期望 {etag}，实际 {actual}），已保留文件现场"
        )));
    }
    Ok(())
}

/// 将内容先写入同目录 `.part` 文件。恢复模式下，若存在该文件，使用 Range 从
/// 已写入偏移继续；完成后再替换最终文件。失败时保留 `.part` 供下次恢复。
#[allow(clippy::too_many_arguments)]
pub async fn download_object_resumable(
    client: &Client,
    bucket: &str,
    key: &str,
    dest: &Path,
    progress: ProgressFn,
    cancel: Arc<AtomicBool>,
    throttle: Option<Arc<Throttle>>,
    resume: bool,
) -> Result<(), CoreError> {
    if let Some(dir) = dest.parent() {
        if !dir.as_os_str().is_empty() {
            tokio::fs::create_dir_all(dir)
                .await
                .map_err(|e| CoreError::io(format!("创建目录失败 {}", dir.display()), e))?;
        }
    }

    let partial = partial_path(dest);
    let mut offset = if resume {
        tokio::fs::metadata(&partial)
            .await
            .map(|meta| meta.len())
            .unwrap_or(0)
    } else {
        0
    };
    let (out, offset) = loop {
        let mut request = client.get_object().bucket(bucket).key(key);
        if offset > 0 {
            request = request.range(format!("bytes={offset}-"));
        }
        let out = request.send().await.map_err(|e| classify(&e))?;
        // 部分 S3 兼容服务会忽略 Range 并返回完整对象；此时不能 append，否则会得到
        // 损坏文件。删除临时文件后以普通请求重新开始。
        if offset > 0 && out.content_range().is_none() {
            let _ = tokio::fs::remove_file(&partial).await;
            offset = 0;
            continue;
        }
        break (out, offset);
    };
    let total = out.content_length().unwrap_or(0).max(0) as u64 + offset;

    let result = stream_to_file(
        out.body.into_async_read(),
        &partial,
        total,
        offset,
        &progress,
        &cancel,
        throttle,
    )
    .await;
    if matches!(result, Err(CoreError::Cancelled)) {
        let _ = tokio::fs::remove_file(&partial).await;
    }
    result?;
    tokio::fs::rename(&partial, dest)
        .await
        .map_err(|e| CoreError::io(format!("完成下载失败 {}", dest.display()), e))
}

pub fn partial_path(dest: &Path) -> std::path::PathBuf {
    let mut path = dest.as_os_str().to_os_string();
    path.push(".part");
    path.into()
}

#[allow(clippy::too_many_arguments)]
async fn stream_to_file<R: tokio::io::AsyncRead + Unpin>(
    mut reader: R,
    dest: &Path,
    total: u64,
    offset: u64,
    progress: &ProgressFn,
    cancel: &Arc<AtomicBool>,
    throttle: Option<Arc<Throttle>>,
) -> Result<(), CoreError> {
    let mut opts = tokio::fs::OpenOptions::new();
    opts.create(true).write(true);
    if offset == 0 {
        opts.truncate(true);
    } else {
        opts.append(true);
    }
    let mut file = opts
        .open(dest)
        .await
        .map_err(|e| CoreError::io(format!("创建文件失败 {}", dest.display()), e))?;
    let mut buf = vec![0u8; 256 * 1024];
    let mut done = offset;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(CoreError::Cancelled);
        }
        let n = reader
            .read(&mut buf)
            .await
            .map_err(|e| CoreError::io("下载数据读取失败", e))?;
        if n == 0 {
            break;
        }
        if let Some(throttle) = throttle.as_ref() {
            throttle.acquire(n as u64).await;
            if cancel.load(Ordering::Relaxed) {
                return Err(CoreError::Cancelled);
            }
        }
        file.write_all(&buf[..n])
            .await
            .map_err(|e| CoreError::io(format!("写入文件失败 {}", dest.display()), e))?;
        done += n as u64;
        progress(done, total);
    }
    file.flush()
        .await
        .map_err(|e| CoreError::io(format!("写入文件失败 {}", dest.display()), e))?;
    Ok(())
}
