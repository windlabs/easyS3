//! P2-05 S3 Inventory CSV 的本地轻量索引。
use crate::error::CoreError;
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryEntry {
    pub bucket: String,
    pub key: String,
    pub size: u64,
    pub last_modified: String,
    pub storage_class: String,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InventoryIndex {
    pub entries: Vec<InventoryEntry>,
}
pub fn import_csv(path: &Path) -> Result<InventoryIndex, CoreError> {
    let text = fs::read_to_string(path).map_err(|e| CoreError::io("读取 Inventory CSV 失败", e))?;
    let mut entries = Vec::new();
    for line in text.lines() {
        let fields: Vec<_> = line.split(',').collect();
        if fields.len() < 12 || fields[0] == "Bucket" {
            continue;
        }
        entries.push(InventoryEntry {
            bucket: fields[0].to_string(),
            key: fields[1].to_string(),
            size: fields[3].parse().unwrap_or(0),
            last_modified: fields[4].to_string(),
            storage_class: fields[9].to_string(),
        });
    }
    Ok(InventoryIndex { entries })
}
pub fn search(index: &InventoryIndex, needle: &str) -> Vec<InventoryEntry> {
    let needle = needle.to_ascii_lowercase();
    index
        .entries
        .iter()
        .filter(|entry| entry.key.to_ascii_lowercase().contains(&needle))
        .cloned()
        .collect()
}
pub fn save(path: &Path, index: &InventoryIndex) -> Result<(), CoreError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| CoreError::io("创建索引目录失败", e))?;
    }
    fs::write(
        path,
        serde_json::to_string(index)
            .map_err(|e| CoreError::Other(format!("序列化索引失败：{e}")))?,
    )
    .map_err(|e| CoreError::io("保存索引失败", e))
}
pub fn load(path: &Path) -> Result<InventoryIndex, CoreError> {
    let text = fs::read_to_string(path).map_err(|e| CoreError::io("读取索引失败", e))?;
    serde_json::from_str(&text).map_err(|_| CoreError::Other("本地索引格式无效".into()))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_and_searches() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("i.csv");
        fs::write(&p,"bucket,key,version,size,last,etag,islatest,delete,storage,sc,owner,acl\nb,k/test.log,,12,now,,,,STANDARD,,,\n").unwrap();
        let index = import_csv(&p).unwrap();
        assert_eq!(search(&index, "test").len(), 1);
    }
}
