//! 删除：批量 DeleteObjects（单请求上限 1000，超出自动分批），逐对象报告失败。
//! 规格：`.agents/s3-operations.md`「4. 删除」。

use crate::error::{classify, CoreError};
use aws_sdk_s3::types::{Delete, ObjectIdentifier};
use aws_sdk_s3::Client;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// DeleteObjects 单请求对象上限（S3 限制）
pub const BATCH_SIZE: usize = 1000;

#[derive(Debug, Clone, Serialize)]
pub struct DeleteFailure {
    pub key: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct DeleteReport {
    pub deleted: usize,
    pub failures: Vec<DeleteFailure>,
}

/// 批量删除。quiet 模式下 S3 只返回失败项；已成功的不回滚（规格）。
pub async fn delete_keys(
    client: &Client,
    bucket: &str,
    keys: &[String],
    cancel: Arc<AtomicBool>,
) -> Result<DeleteReport, CoreError> {
    let mut report = DeleteReport::default();
    for chunk in keys.chunks(BATCH_SIZE) {
        if cancel.load(Ordering::Relaxed) {
            return Err(CoreError::Cancelled);
        }
        let objects: Vec<ObjectIdentifier> = chunk
            .iter()
            .map(|k| {
                ObjectIdentifier::builder()
                    .key(k)
                    .build()
                    .expect("key 必填")
            })
            .collect();
        let delete = Delete::builder()
            .set_objects(Some(objects))
            .quiet(true)
            .build()
            .map_err(|e| CoreError::Other(format!("构造删除请求失败：{e}")))?;

        let out = client
            .delete_objects()
            .bucket(bucket)
            .delete(delete)
            .send()
            .await
            .map_err(|e| classify(&e))?;

        for err in out.errors() {
            report.failures.push(DeleteFailure {
                key: err.key().unwrap_or_default().to_string(),
                message: format!(
                    "{} {}",
                    err.code().unwrap_or_default(),
                    err.message().unwrap_or_default()
                )
                .trim()
                .to_string(),
            });
        }
        report.deleted += chunk.len() - out.errors().len();
    }
    Ok(report)
}
