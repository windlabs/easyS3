//! 传输设置模型与持久化（settings.json）：自动重试、限速、并发、分片。
//! 规格：`.agents/requirements.md`「传输设置」、`.agents/s3-operations.md` 通用约束。
//!
//! 与 connections.json 同目录、同原子写策略（tmp + rename）；
//! 字段缺失 / 旧版本文件 / 越界值均回落到默认并收敛到允许区间。

use crate::preview::DEFAULT_PREVIEW_LIMIT_MB;
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const DEFAULT_AUTO_RETRY_COUNT: u32 = 3;
pub const DEFAULT_FILE_CONCURRENCY: u32 = 2;
pub const DEFAULT_PART_SIZE_MB: u32 = 8;
pub const DEFAULT_MAX_CONCURRENT_TASKS: u32 = 3;

pub const MIN_PART_SIZE_MB: u32 = 5;
pub const MAX_PART_SIZE_MB: u32 = 64;
pub const MAX_FILE_CONCURRENCY: u32 = 8;
pub const MAX_CONCURRENT_TASKS: u32 = 16;
pub const MAX_AUTO_RETRY_COUNT: u32 = 10;
/// PDF / 音视频预览上限范围（MB）
pub const MIN_PREVIEW_LIMIT_MB: u32 = 1;
pub const MAX_PREVIEW_LIMIT_MB: u32 = 100;
/// 限速上限 1 GB/s，防止溢出与误输入
pub const MAX_LIMIT_KBPS: u64 = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TransferSettings {
    /// 瞬时错误自动重试次数（0 = 关闭）
    pub auto_retry_count: u32,
    /// 全局上传限速 KB/s（0 = 不限速）
    pub upload_limit_kbps: u64,
    /// 全局下载限速 KB/s（0 = 不限速）
    pub download_limit_kbps: u64,
    /// 单任务文件间并发数
    pub file_concurrency: u32,
    /// multipart 分片大小（MB，≥5）
    pub part_size_mb: u32,
    /// 最大并发任务数（超出排队）
    pub max_concurrent_tasks: u32,
    /// PDF / 音视频预览大小上限（MB）
    pub preview_limit_mb: u32,
}

impl Default for TransferSettings {
    fn default() -> Self {
        TransferSettings {
            auto_retry_count: DEFAULT_AUTO_RETRY_COUNT,
            upload_limit_kbps: 0,
            download_limit_kbps: 0,
            file_concurrency: DEFAULT_FILE_CONCURRENCY,
            part_size_mb: DEFAULT_PART_SIZE_MB,
            max_concurrent_tasks: DEFAULT_MAX_CONCURRENT_TASKS,
            preview_limit_mb: DEFAULT_PREVIEW_LIMIT_MB as u32,
        }
    }
}

impl TransferSettings {
    /// 越界值收敛到允许区间（旧配置 / 手改文件容错）
    pub fn sanitized(self) -> Self {
        TransferSettings {
            auto_retry_count: self.auto_retry_count.min(MAX_AUTO_RETRY_COUNT),
            upload_limit_kbps: self.upload_limit_kbps.min(MAX_LIMIT_KBPS),
            download_limit_kbps: self.download_limit_kbps.min(MAX_LIMIT_KBPS),
            file_concurrency: self.file_concurrency.clamp(1, MAX_FILE_CONCURRENCY),
            part_size_mb: self.part_size_mb.clamp(MIN_PART_SIZE_MB, MAX_PART_SIZE_MB),
            max_concurrent_tasks: self.max_concurrent_tasks.clamp(1, MAX_CONCURRENT_TASKS),
            preview_limit_mb: self
                .preview_limit_mb
                .clamp(MIN_PREVIEW_LIMIT_MB, MAX_PREVIEW_LIMIT_MB),
        }
    }

    pub fn part_size_bytes(&self) -> u64 {
        self.part_size_mb as u64 * 1024 * 1024
    }

    /// PDF / 音视频预览上限（字节）
    pub fn preview_limit_bytes(&self) -> i64 {
        self.preview_limit_mb as i64 * 1024 * 1024
    }
}

/// 读设置；文件缺失 / 损坏 / 非法值均回落默认（不阻断启动）。
pub fn load_settings(path: &Path) -> TransferSettings {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<TransferSettings>(&text).ok())
        .unwrap_or_default()
        .sanitized()
}

/// 原子写：tmp + rename（与 connections.json 一致）。
pub fn save_settings(path: &Path, settings: &TransferSettings) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("创建设置目录失败：{e}"))?;
    }
    let text =
        serde_json::to_string_pretty(settings).map_err(|e| format!("序列化设置失败：{e}"))?;
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, text).map_err(|e| format!("写入设置失败：{e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("保存设置失败：{e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_returns_default() {
        let dir = tempfile::tempdir().unwrap();
        let s = load_settings(&dir.path().join("settings.json"));
        assert_eq!(s, TransferSettings::default());
    }

    #[test]
    fn corrupted_file_returns_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, "not json").unwrap();
        assert_eq!(load_settings(&path), TransferSettings::default());
    }

    #[test]
    fn missing_fields_use_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, r#"{"upload_limit_kbps": 512}"#).unwrap();
        let s = load_settings(&path);
        assert_eq!(s.upload_limit_kbps, 512);
        assert_eq!(s.auto_retry_count, DEFAULT_AUTO_RETRY_COUNT);
        assert_eq!(s.file_concurrency, DEFAULT_FILE_CONCURRENCY);
    }

    #[test]
    fn out_of_range_values_are_clamped() {
        let s = TransferSettings {
            auto_retry_count: 99,
            upload_limit_kbps: u64::MAX,
            file_concurrency: 0,
            part_size_mb: 1,
            max_concurrent_tasks: 100,
            preview_limit_mb: 0,
            ..TransferSettings::default()
        }
        .sanitized();
        assert_eq!(s.auto_retry_count, MAX_AUTO_RETRY_COUNT);
        assert_eq!(s.upload_limit_kbps, MAX_LIMIT_KBPS);
        assert_eq!(s.file_concurrency, 1);
        assert_eq!(s.part_size_mb, MIN_PART_SIZE_MB);
        assert_eq!(s.max_concurrent_tasks, MAX_CONCURRENT_TASKS);
        assert_eq!(s.preview_limit_mb, MIN_PREVIEW_LIMIT_MB);
    }

    #[test]
    fn save_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let s = TransferSettings {
            auto_retry_count: 5,
            upload_limit_kbps: 2048,
            download_limit_kbps: 1024,
            file_concurrency: 4,
            part_size_mb: 16,
            max_concurrent_tasks: 6,
            preview_limit_mb: 50,
        };
        save_settings(&path, &s).unwrap();
        assert_eq!(load_settings(&path), s);
    }

    #[test]
    fn part_size_bytes_conversion() {
        let s = TransferSettings {
            part_size_mb: 16,
            ..TransferSettings::default()
        };
        assert_eq!(s.part_size_bytes(), 16 * 1024 * 1024);
    }
}
