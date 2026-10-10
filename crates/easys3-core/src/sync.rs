//! P1-07 单向同步计划的纯数据模型与本地侧比较逻辑。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncDirection {
    Upload,
    Download,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncPlanItem {
    pub source: String,
    pub target: String,
    pub action: String,
    pub size: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SyncPlan {
    pub create: Vec<SyncPlanItem>,
    pub update: Vec<SyncPlanItem>,
    pub delete: Vec<SyncPlanItem>,
    pub skipped: usize,
}

fn walk_local(
    root: &Path,
    dir: &Path,
    files: &mut Vec<(PathBuf, String, u64)>,
) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let kind = entry.file_type()?;
        if kind.is_dir() {
            walk_local(root, &path, files)?;
        } else if kind.is_file() {
            let relative = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            files.push((path, relative, entry.metadata()?.len()));
        }
    }
    Ok(())
}

/// 本地→S3：远端相同大小跳过，否则产生创建或更新计划；可选删除仅返回计划，不执行。
pub fn plan_upload(
    local_dir: &Path,
    prefix: &str,
    remote_sizes: &HashMap<String, u64>,
    propagate_delete: bool,
) -> std::io::Result<SyncPlan> {
    let mut files = Vec::new();
    walk_local(local_dir, local_dir, &mut files)?;
    let mut plan = SyncPlan::default();
    let mut seen = std::collections::HashSet::new();
    for (path, rel, size) in files {
        let key = format!("{prefix}{rel}");
        seen.insert(key.clone());
        let item = SyncPlanItem {
            source: path.to_string_lossy().into_owned(),
            target: key.clone(),
            action: "upload".into(),
            size,
        };
        match remote_sizes.get(&key) {
            None => plan.create.push(item),
            Some(old) if *old != size => plan.update.push(item),
            Some(_) => plan.skipped += 1,
        }
    }
    if propagate_delete {
        for (key, size) in remote_sizes {
            if !seen.contains(key) {
                plan.delete.push(SyncPlanItem {
                    source: key.clone(),
                    target: String::new(),
                    action: "delete_remote".into(),
                    size: *size,
                });
            }
        }
    }
    Ok(plan)
}

/// S3→本地：以远端 prefix 的相对 key 映射本地路径；同大小文件跳过。
pub fn plan_download(
    local_dir: &Path,
    prefix: &str,
    remote_sizes: &HashMap<String, u64>,
    propagate_delete: bool,
) -> std::io::Result<SyncPlan> {
    let mut local = Vec::new();
    walk_local(local_dir, local_dir, &mut local)?;
    let local_sizes: HashMap<String, u64> = local
        .into_iter()
        .map(|(_, rel, size)| (rel, size))
        .collect();
    let mut plan = SyncPlan::default();
    for (key, size) in remote_sizes {
        let rel = key.strip_prefix(prefix).unwrap_or(key).to_string();
        let item = SyncPlanItem {
            source: key.clone(),
            target: local_dir.join(&rel).to_string_lossy().into_owned(),
            action: "download".into(),
            size: *size,
        };
        match local_sizes.get(&rel) {
            None => plan.create.push(item),
            Some(old) if *old != *size => plan.update.push(item),
            Some(_) => plan.skipped += 1,
        }
    }
    if propagate_delete {
        for (rel, size) in local_sizes {
            let key = format!("{prefix}{rel}");
            if !remote_sizes.contains_key(&key) {
                plan.delete.push(SyncPlanItem {
                    source: rel.clone(),
                    target: local_dir.join(rel).to_string_lossy().into_owned(),
                    action: "delete_local".into(),
                    size,
                });
            }
        }
    }
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn upload_plan_distinguishes_create_update_skip() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("new.txt"), b"a").unwrap();
        std::fs::write(dir.path().join("old.txt"), b"xx").unwrap();
        let remote = HashMap::from([("p/old.txt".into(), 1), ("p/same.txt".into(), 1)]);
        let plan = plan_upload(dir.path(), "p/", &remote, true).unwrap();
        assert_eq!(plan.create.len(), 1);
        assert_eq!(plan.update.len(), 1);
        assert_eq!(plan.delete.len(), 1);
    }
    #[test]
    fn download_plan_maps_relative_paths() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("same"), b"a").unwrap();
        let remote = HashMap::from([("p/same".into(), 1), ("p/new/x".into(), 2)]);
        let plan = plan_download(dir.path(), "p/", &remote, false).unwrap();
        assert_eq!(plan.create.len(), 1);
        assert!(plan.create[0].target.ends_with("new/x"));
        assert_eq!(plan.skipped, 1);
    }
}
