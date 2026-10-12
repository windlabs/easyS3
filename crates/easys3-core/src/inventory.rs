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
    let mut entries = Vec::new();
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_path(path)
        .map_err(|e| CoreError::Other(format!("读取 Inventory CSV 失败：{e}")))?;
    for row in reader.records() {
        let row = row.map_err(|e| CoreError::Other(format!("解析 Inventory CSV 失败：{e}")))?;
        if row.len() < 10 || row.get(0) == Some("Bucket") {
            continue;
        }
        entries.push(InventoryEntry {
            bucket: row.get(0).unwrap_or_default().to_string(),
            key: row.get(1).unwrap_or_default().to_string(),
            size: row.get(3).unwrap_or_default().parse().unwrap_or(0),
            last_modified: row.get(4).unwrap_or_default().to_string(),
            storage_class: row.get(9).unwrap_or_default().to_string(),
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
        fs::write(&p,"bucket,key,version,size,last,etag,islatest,delete,storage,sc,owner,acl\nb,\"k,test.log\",,12,now,,,,STANDARD,,,\n").unwrap();
        let index = import_csv(&p).unwrap();
        assert_eq!(search(&index, "k,test").len(), 1);
    }
}
