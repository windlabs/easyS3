//! P2-08 一期：本机操作结果历史（不恢复执行）。
use crate::error::CoreError;
use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub at: u64,
    pub connection: String,
    pub bucket: String,
    pub action: String,
    pub status: String,
    pub detail: String,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuditFile {
    #[serde(default)]
    pub entries: Vec<AuditEntry>,
}
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
pub fn load(path: &Path) -> Result<AuditFile, CoreError> {
    match fs::read_to_string(path) {
        Ok(text) => {
            serde_json::from_str(&text).map_err(|_| CoreError::Other("操作历史文件格式无效".into()))
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(AuditFile::default()),
        Err(e) => Err(CoreError::io("读取操作历史失败", e)),
    }
}
pub fn save(path: &Path, file: &AuditFile) -> Result<(), CoreError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| CoreError::io("创建操作历史目录失败", e))?;
    }
    fs::write(
        path,
        serde_json::to_string_pretty(file)
            .map_err(|e| CoreError::Other(format!("序列化操作历史失败：{e}")))?,
    )
    .map_err(|e| CoreError::io("保存操作历史失败", e))
}
