//! 纯本地文件规划逻辑：上传收集、下载冲突命名。
//! 规格：`.agents/s3-operations.md`「2. 上传」「3. 下载」。

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

/// 上传计划条目：(本地路径, 目标 key)
pub type PlannedUpload = (PathBuf, String);

/// 收集要上传的文件：文件直接挂到 prefix 下；文件夹递归展开保留相对目录结构
/// （key = 当前前缀 + 相对路径，规格）。跳过符号链接（避免循环）。
pub fn collect_upload_files(paths: &[PathBuf], prefix: &str) -> Vec<PlannedUpload> {
    let mut out = Vec::new();
    for p in paths {
        match fs::metadata(p) {
            Ok(m) if m.is_file() => {
                if let Some(name) = p.file_name() {
                    out.push((p.clone(), join_key(prefix, &name.to_string_lossy())));
                }
            }
            Ok(m) if m.is_dir() => walk_dir(p, p, prefix, &mut out),
            _ => {} // 不存在或不可访问：跳过
        }
    }
    out.sort_by(|a, b| a.1.cmp(&b.1));
    out
}

fn walk_dir(dir: &Path, base: &Path, prefix: &str, out: &mut Vec<PlannedUpload>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for entry in rd.flatten() {
        let Ok(ft) = entry.file_type() else { continue };
        if ft.is_symlink() {
            continue;
        }
        let path = entry.path();
        if ft.is_dir() {
            walk_dir(&path, base, prefix, out);
        } else {
            let rel = path.strip_prefix(base).unwrap_or(&path);
            let rel_key = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            if !rel_key.is_empty() {
                out.push((path.clone(), join_key(prefix, &rel_key)));
            }
        }
    }
}

/// prefix 必须以 / 结尾后拼接 key；统一使用 / 作为 key 分隔符。
pub fn join_key(prefix: &str, rel: &str) -> String {
    let trimmed = prefix.trim_end_matches('/');
    if trimmed.is_empty() {
        rel.to_string()
    } else {
        format!("{trimmed}/{rel}")
    }
}

/// 下载同名冲突自动重命名：`name (1).ext`（规格）。
pub fn unique_path(dest: &Path) -> PathBuf {
    if !dest.exists() {
        return dest.to_path_buf();
    }
    let parent = dest.parent();
    let stem = dest
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let ext = dest
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    for i in 1..u32::MAX {
        let cand_name = format!("{stem} ({i}){ext}");
        let cand = match parent {
            Some(p) => p.join(cand_name),
            None => PathBuf::from(cand_name),
        };
        if !cand.exists() {
            return cand;
        }
    }
    dest.to_path_buf()
}

// ---------- 重试范围判定（规格：重试精确命中失败项） ----------

/// 判定对象 key 是否落在失败项的重试范围内。
/// 失败项两种形态：
/// - 对象 key：精确匹配（传输失败、请求级删除失败的未确认对象）；
/// - 目录前缀（以 `/` 结尾）：展开阶段列出失败——命中其下全部对象，重试时重新展开。
///
/// 前缀匹配要求失败项以 `/` 结尾，避免 "dir/a.txt" 误命中 "dir/a.txt.bak"。
pub fn key_matches_failure(key: &str, failed: &HashSet<String>) -> bool {
    failed
        .iter()
        .any(|f| key == f.as_str() || (f.ends_with('/') && key.starts_with(f.as_str())))
}

/// 判定列表项（文件 / 目录）是否需要参与重试。
/// 文件：key 精确命中失败项；目录（key 为以 `/` 结尾的前缀）：自身展开失败（key 命中）
/// 或其下有失败对象（存在以该前缀开头的失败项）。
pub fn item_matches_failure(key: &str, is_dir: bool, failed: &HashSet<String>) -> bool {
    if is_dir {
        failed.contains(key) || failed.iter().any(|f| f.starts_with(key))
    } else {
        failed.contains(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_key_variants() {
        assert_eq!(join_key("", "a.txt"), "a.txt");
        assert_eq!(join_key("dir/", "a.txt"), "dir/a.txt");
        assert_eq!(join_key("dir//", "sub/a.txt"), "dir/sub/a.txt");
        assert_eq!(join_key("dir", "a.txt"), "dir/a.txt");
    }

    #[test]
    fn unique_path_renames() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("f.txt");
        fs::write(&a, "x").unwrap();
        let b = unique_path(&a);
        assert_eq!(b.file_name().unwrap().to_string_lossy(), "f (1).txt");
        fs::write(&b, "x").unwrap();
        let c = unique_path(&a);
        assert_eq!(c.file_name().unwrap().to_string_lossy(), "f (2).txt");
        let fresh = dir.path().join("new.bin");
        assert_eq!(unique_path(&fresh), fresh);
    }

    #[test]
    fn retry_matching_exact_and_prefix() {
        let mut failed = HashSet::new();
        failed.insert("dir/a.txt".to_string()); // 对象失败
        failed.insert("d2/".to_string()); // 目录展开失败
                                          // 精确命中
        assert!(key_matches_failure("dir/a.txt", &failed));
        assert!(item_matches_failure("dir/a.txt", false, &failed));
        // 前缀命中：展开失败的目录，其下全部对象
        assert!(key_matches_failure("d2/x.txt", &failed));
        assert!(key_matches_failure("d2/", &failed));
        assert!(item_matches_failure("d2/", true, &failed));
        // 同目录其他对象 / 无关对象不命中
        assert!(!key_matches_failure("dir/b.txt", &failed));
        assert!(!key_matches_failure("other", &failed));
        // 非前缀失败项不做前缀匹配（dir/a.txt ≠ dir/a.txt.bak）
        assert!(!key_matches_failure("dir/a.txt.bak", &failed));
        // 前缀边界：d2/ 不含 d22/x
        assert!(!key_matches_failure("d22/x", &failed));
        // 目录下有对象失败 → 该目录参与重试（重新展开后仅重试失败对象）
        let mut f2 = HashSet::new();
        f2.insert("dir/a.txt".to_string());
        assert!(item_matches_failure("dir/", true, &f2));
        assert!(!item_matches_failure("dir2/", true, &f2));
        assert!(!item_matches_failure("b.txt", false, &f2));
    }

    #[test]
    fn collect_files_and_dirs() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), "x").unwrap();
        let sub = dir.path().join("sub");
        fs::create_dir(&sub).unwrap();
        fs::write(sub.join("b.log"), "y").unwrap();
        let deep = sub.join("deep");
        fs::create_dir(&deep).unwrap();
        fs::write(deep.join("中文 文件.csv"), "z").unwrap();

        // 单文件
        let single = collect_upload_files(&[dir.path().join("a.txt")], "top/");
        assert_eq!(single.len(), 1);
        assert_eq!(single[0].1, "top/a.txt");

        // 文件夹递归，保留结构，含中文与空格文件名
        let all = collect_upload_files(&[dir.path().to_path_buf()], "top");
        let keys: Vec<_> = all.iter().map(|(_, k)| k.as_str()).collect();
        assert!(keys.contains(&"top/sub/b.log"));
        assert!(keys.contains(&"top/sub/deep/中文 文件.csv"));
        assert_eq!(all.len(), 3);
    }
}
