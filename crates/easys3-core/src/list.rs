//! 列表查询：桶列表、对象列表（虚拟目录 + 分页）。
//! 规格：`.agents/s3-operations.md`「1. 列表查询」。

use crate::error::{classify, CoreError};
use aws_sdk_s3::Client;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// S3 单页上限（ListObjectsV2 max_keys 最大值，规格：分页每页 1000）。
pub const PAGE_SIZE: i32 = 1000;

#[derive(Debug, Clone, Serialize)]
pub struct Entry {
    /// 完整 key（目录为前缀，以 / 结尾）
    pub key: String,
    /// 显示名（最后一段）
    pub name: String,
    pub size: u64,
    pub last_modified: Option<String>,
    pub is_dir: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ListResult {
    pub entries: Vec<Entry>,
    pub next_token: Option<String>,
}

pub async fn list_buckets(client: &Client) -> Result<Vec<String>, CoreError> {
    let out = client
        .list_buckets()
        .send()
        .await
        .map_err(|e| classify(&e))?;
    Ok(out
        .buckets()
        .iter()
        .filter_map(|b| b.name().map(|n| n.to_string()))
        .collect())
}

/// 列对象：delimiter='/' 产生 CommonPrefixes 虚拟目录；滚动分页用 continuation token。
/// 禁止一次性拉全量（规格）。
pub async fn list_objects(
    client: &Client,
    bucket: &str,
    prefix: &str,
    token: Option<&str>,
) -> Result<ListResult, CoreError> {
    let mut req = client
        .list_objects_v2()
        .bucket(bucket)
        .prefix(prefix)
        .max_keys(PAGE_SIZE)
        .delimiter("/");
    if let Some(t) = token {
        req = req.continuation_token(t);
    }
    let out = req.send().await.map_err(|e| classify(&e))?;

    let mut dirs: Vec<Entry> = out
        .common_prefixes()
        .iter()
        .filter_map(|cp| {
            cp.prefix().map(|p| Entry {
                key: p.to_string(),
                name: name_of(p),
                size: 0,
                last_modified: None,
                is_dir: true,
            })
        })
        .collect();

    let mut objects: Vec<Entry> = Vec::new();
    for o in out.contents() {
        let key = o.key().unwrap_or_default();
        if key.is_empty() || key == prefix {
            continue;
        }
        let size = o.size().unwrap_or(0).max(0) as u64;
        // key 以 / 结尾的 0 字节占位对象按“文件夹”渲染（规格）
        if key.ends_with('/') && size == 0 {
            dirs.push(Entry {
                key: key.to_string(),
                name: name_of(key),
                size: 0,
                last_modified: o.last_modified().map(|d| d.to_string()),
                is_dir: true,
            });
        } else {
            objects.push(Entry {
                key: key.to_string(),
                name: name_of(key),
                size,
                last_modified: o.last_modified().map(|d| d.to_string()),
                is_dir: false,
            });
        }
    }

    // 占位对象与 CommonPrefixes 去重；默认名称升序、文件夹在前（规格）
    dirs.sort_by(|a, b| a.key.cmp(&b.key));
    dirs.dedup_by(|a, b| a.key == b.key);
    objects.sort_by(|a, b| a.key.cmp(&b.key));
    dirs.extend(objects);

    Ok(ListResult {
        entries: dirs,
        next_token: out.next_continuation_token().map(|t| t.to_string()),
    })
}

/// 递归列出前缀下全部对象 key（用于文件夹下载/删除/统计）。
///
/// **不带 delimiter** 分页拉取，天然覆盖任意子层级（规格：递归列出全部对象；
/// 旧实现复用带 delimiter 的列表查询，深层对象永远不会被列出，已修复）。
/// `include_dir_placeholders = true` 时包含目录占位对象（删除前缀时须随前缀
/// 一并删除，规格 §1/§4）；下载传 `false`（占位对象不可下载，规格 §3）。
pub async fn collect_prefix_objects(
    client: &Client,
    bucket: &str,
    prefix: &str,
    cancel: &Arc<AtomicBool>,
    include_dir_placeholders: bool,
) -> Result<Vec<String>, CoreError> {
    let mut keys = Vec::new();
    let mut token: Option<String> = None;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(CoreError::Cancelled);
        }
        let mut req = client
            .list_objects_v2()
            .bucket(bucket)
            .prefix(prefix)
            .max_keys(PAGE_SIZE);
        if let Some(t) = &token {
            req = req.continuation_token(t);
        }
        let out = req.send().await.map_err(|e| classify(&e))?;
        for o in out.contents() {
            let key = o.key().unwrap_or_default();
            if key.is_empty() {
                continue;
            }
            let size = o.size().unwrap_or(0).max(0);
            if !include_dir_placeholders && is_dir_marker(key, size) {
                continue;
            }
            keys.push(key.to_string());
        }
        token = out.next_continuation_token().map(|t| t.to_string());
        if token.is_none() {
            return Ok(keys);
        }
    }
}

/// 目录占位对象：key 以 `/` 结尾且 0 字节（UI 渲染为文件夹，规格 §1）。
/// 与 `list_objects` 的渲染判定保持一致：非 0 字节的 `/` 结尾 key 仍按对象处理。
pub fn is_dir_marker(key: &str, size: i64) -> bool {
    key.ends_with('/') && size == 0
}

/// key 的显示名：去掉结尾的 / 后取最后一段。支持中文/空格/Unicode（规格）。
pub fn name_of(key: &str) -> String {
    let trimmed = key.trim_end_matches('/');
    if trimmed.is_empty() {
        return key.to_string();
    }
    trimmed.rsplit('/').next().unwrap_or(trimmed).to_string()
}

#[cfg(test)]
mod tests {
    use super::{is_dir_marker, name_of};

    #[test]
    fn name_of_variants() {
        assert_eq!(name_of("a/b/c.txt"), "c.txt");
        assert_eq!(name_of("dir/"), "dir");
        assert_eq!(name_of("dir//"), "dir");
        assert_eq!(name_of("单个文件.log"), "单个文件.log");
        assert_eq!(name_of("带 空格/文件 名.tar.gz"), "文件 名.tar.gz");
        assert_eq!(name_of("root"), "root");
    }

    #[test]
    fn dir_marker_rules() {
        // 占位对象：/ 结尾且 0 字节（与 list_objects 渲染判定一致）
        assert!(is_dir_marker("a/", 0));
        assert!(is_dir_marker("a/b/", 0));
        // 非 0 字节的 / 结尾 key 按普通对象处理（可下载）
        assert!(!is_dir_marker("a/", 128));
        // 普通对象
        assert!(!is_dir_marker("a/b.txt", 0));
        assert!(!is_dir_marker("a", 0));
    }
}
