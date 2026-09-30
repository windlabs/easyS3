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

/// 批量删除结果：报告 + 可选的请求级错误（规格 §4）。
///
/// 部分成功也如实报告——已完成批次的 `deleted` 与逐对象失败保留，
/// 供任务进度展示与「重试失败项」精确命中使用。
#[derive(Debug, Clone, Default)]
pub struct DeleteOutcome {
    pub report: DeleteReport,
    /// Some = 批量请求中断（取消 / 网络 / 服务错误）；
    /// 此时该批及其后未执行的对象已逐个记入 failures（删除结果未知，可重试）
    pub error: Option<CoreError>,
}

/// 批量删除。quiet 模式下 S3 只返回失败项；已成功的不回滚（规格）。
/// 请求级失败时不丢弃已完成批次的进度（见 [`DeleteOutcome`]）。
pub async fn delete_keys(
    client: &Client,
    bucket: &str,
    keys: &[String],
    cancel: Arc<AtomicBool>,
) -> DeleteOutcome {
    let mut outcome = DeleteOutcome::default();
    // 空 key 会令 ObjectIdentifier 构造失败（原实现的 expect 会 panic 挂死任务线程），跳过
    let keys: Vec<&String> = keys.iter().filter(|k| !k.is_empty()).collect();

    let mut idx = 0usize;
    while idx < keys.len() {
        if cancel.load(Ordering::Relaxed) {
            outcome.error = Some(CoreError::Cancelled);
            return outcome;
        }
        let end = (idx + BATCH_SIZE).min(keys.len());
        let chunk = &keys[idx..end];
        let objects: Vec<ObjectIdentifier> = chunk
            .iter()
            .map(|k| {
                ObjectIdentifier::builder()
                    .key(*k)
                    .build()
                    .expect("非空 key 构造必然成功")
            })
            .collect();
        let delete = match Delete::builder()
            .set_objects(Some(objects))
            .quiet(true)
            .build()
        {
            Ok(d) => d,
            Err(e) => {
                mark_rest_failed(&mut outcome, &keys[idx..], &e.to_string());
                outcome.error = Some(CoreError::Other(format!("构造删除请求失败：{e}")));
                return outcome;
            }
        };

        let out = match client
            .delete_objects()
            .bucket(bucket)
            .delete(delete)
            .send()
            .await
        {
            Ok(o) => o,
            Err(e) => {
                // 请求级失败：结果未知。该批及其后未执行对象逐个记入失败，
                // 保证「重试失败项」可命中（S3 删除幂等，重复删除安全，规格）
                let err = classify(&e);
                mark_rest_failed(&mut outcome, &keys[idx..], &err.user_message());
                outcome.error = Some(err);
                return outcome;
            }
        };

        for err in out.errors() {
            outcome.report.failures.push(DeleteFailure {
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
        outcome.report.deleted += chunk.len() - out.errors().len();
        idx = end;
    }
    outcome
}

/// 请求中断时把剩余对象逐个记入失败（真实 key，供重试精确匹配）。
fn mark_rest_failed(outcome: &mut DeleteOutcome, rest: &[&String], reason: &str) {
    for k in rest {
        outcome.report.failures.push(DeleteFailure {
            key: (*k).clone(),
            message: format!("{reason}（该对象删除结果未知，可重试）"),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 请求级失败的剩余对象必须记真实 key：retry_task 按失败项 key 过滤
    /// 原始 keys 生成重试子集，假 key（如 "-"）会让重试永远落空（规格 §4）。
    #[test]
    fn mark_rest_failed_uses_real_keys_for_retry() {
        let mut outcome = DeleteOutcome::default();
        let keys = ["dir/a.txt".to_string(), "dir/b.txt".to_string()];
        let rest: Vec<&String> = keys.iter().collect();
        mark_rest_failed(&mut outcome, &rest, "无法连接到服务");
        assert_eq!(outcome.report.failures.len(), 2);
        assert_eq!(outcome.report.failures[0].key, "dir/a.txt");
        assert_eq!(outcome.report.failures[1].key, "dir/b.txt");
        assert_eq!(
            outcome.report.failures[0].message,
            "无法连接到服务（该对象删除结果未知，可重试）"
        );
        assert_eq!(outcome.report.deleted, 0);
    }
}
