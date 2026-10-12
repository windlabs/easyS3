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
        .has_headers(true)
        .flexible(true)
        .from_path(path)
        .map_err(|e| CoreError::Other(format!("读取 Inventory CSV 失败：{e}")))?;
    let headers = reader
        .headers()
        .map_err(|e| CoreError::Other(format!("解析 Inventory CSV 表头失败：{e}")))?
        .clone();
    let column = |name: &str, fallback: usize| {
        headers
            .iter()
            .position(|header| header.eq_ignore_ascii_case(name))
            .unwrap_or(fallback)
    };
    let bucket_column = column("Bucket", 0);
    let key_column = column("Key", 1);
    let size_column = column("Size", 3);
    let modified_column = column("LastModifiedDate", 4);
    let storage_column = headers
        .iter()
        .position(|header| {
            header.eq_ignore_ascii_case("StorageClass") || header.eq_ignore_ascii_case("Storage")
        })
        .unwrap_or(8);
    for row in reader.records() {
        let row = row.map_err(|e| CoreError::Other(format!("解析 Inventory CSV 失败：{e}")))?;
        if row.len() < 2 {
            continue;
        }
        entries.push(InventoryEntry {
            bucket: row.get(bucket_column).unwrap_or_default().to_string(),
            key: row.get(key_column).unwrap_or_default().to_string(),
            size: row
                .get(size_column)
                .unwrap_or_default()
                .parse()
                .unwrap_or(0),
            last_modified: row.get(modified_column).unwrap_or_default().to_string(),
            storage_class: row.get(storage_column).unwrap_or_default().to_string(),
        });
    }
    Ok(InventoryIndex { entries })
}
pub fn search(index: &InventoryIndex, query: &str) -> Vec<InventoryEntry> {
    let query = query.trim();
    let mut needle = String::new();
    let mut min_size = None;
    let mut storage_class = None;
    for clause in query.split("AND").map(str::trim) {
        if let Some(value) = clause.strip_prefix("size>") {
            min_size = parse_size(value).ok();
        } else if let Some(value) = clause.strip_prefix("class=") {
            storage_class = Some(value.trim().to_ascii_uppercase());
        } else if !clause.is_empty() {
            needle = clause.to_ascii_lowercase();
        }
    }
    index
        .entries
        .iter()
        .filter(|entry| {
            (needle.is_empty() || entry.key.to_ascii_lowercase().contains(&needle))
                && min_size.is_none_or(|size| entry.size > size)
                && storage_class
                    .as_ref()
                    .is_none_or(|class| entry.storage_class.to_ascii_uppercase() == *class)
        })
        .cloned()
        .collect()
}
fn parse_size(value: &str) -> Result<u64, CoreError> {
    let value = value.trim().to_ascii_uppercase();
    let position = value
        .find(|c: char| c.is_ascii_alphabetic())
        .unwrap_or(value.len());
    let number = value[..position]
        .trim()
        .parse::<u64>()
        .map_err(|_| CoreError::Other("Inventory size 条件格式错误".into()))?;
    let multiplier = match &value[position..] {
        "" | "B" => 1,
        "KB" => 1024,
        "MB" => 1024 * 1024,
        "GB" => 1024 * 1024 * 1024,
        _ => {
            return Err(CoreError::Other(
                "Inventory size 单位仅支持 B/KB/MB/GB".into(),
            ))
        }
    };
    Ok(number.saturating_mul(multiplier))
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
        fs::write(&p,"bucket,key,version,size,last,etag,islatest,delete,storage,sc,owner,acl\nb,\"k,test.log\",,12,now,,,STANDARD,,,,\n").unwrap();
        let index = import_csv(&p).unwrap();
        assert_eq!(search(&index, "k,test").len(), 1);
        assert_eq!(search(&index, "size>10B").len(), 1);
    }
}
