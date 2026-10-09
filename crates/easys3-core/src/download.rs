//! 下载：流式写盘 + 进度事件；取消时删除未完成的分段文件。
//! 规格：`.agents/s3-operations.md`「3. 下载」。

use crate::error::{classify, CoreError};
use crate::throttle::Throttle;
use aws_sdk_s3::Client;
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
    if let Some(dir) = dest.parent() {
        if !dir.as_os_str().is_empty() {
            tokio::fs::create_dir_all(dir)
                .await
                .map_err(|e| CoreError::io(format!("创建目录失败 {}", dir.display()), e))?;
        }
    }

    let out = client
        .get_object()
        .bucket(bucket)
        .key(key)
        .send()
        .await
        .map_err(|e| classify(&e))?;
    let total = out.content_length().unwrap_or(0).max(0) as u64;

    let result = stream_to_file(
        out.body.into_async_read(),
        dest,
        total,
        &progress,
        &cancel,
        throttle,
    )
    .await;

    if result.is_err() {
        // 取消或失败：删除未完成的分段文件（规格；不做断点续传）
        let _ = tokio::fs::remove_file(dest).await;
    }
    result
}

#[allow(clippy::too_many_arguments)]
async fn stream_to_file<R: tokio::io::AsyncRead + Unpin>(
    mut reader: R,
    dest: &Path,
    total: u64,
    progress: &ProgressFn,
    cancel: &Arc<AtomicBool>,
    throttle: Option<Arc<Throttle>>,
) -> Result<(), CoreError> {
    let mut file = tokio::fs::File::create(dest)
        .await
        .map_err(|e| CoreError::io(format!("创建文件失败 {}", dest.display()), e))?;
    let mut buf = vec![0u8; 256 * 1024];
    let mut done: u64 = 0;
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
