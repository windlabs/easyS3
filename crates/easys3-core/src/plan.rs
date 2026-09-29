//! 纯本地文件规划逻辑：上传收集、下载冲突命名。
//! 规格：`.agents/s3-operations.md`「2. 上传」「3. 下载」。

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
