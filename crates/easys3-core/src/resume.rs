//! P1-01 可恢复传输任务的持久化数据模型。
//!
//! 此模块只描述、读写恢复检查点，不依赖 Tauri 或 GUI；凭据、Client、预签名 URL
//! 均不得出现在该文件中。

use crate::error::CoreError;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::Path;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResumeFile {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub tasks: Vec<ResumeTask>,
}

const fn default_version() -> u32 {
    1
}

/// 可恢复任务以连接 id 引用配置，恢复时重新从安全存储读取凭据。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ResumeTask {
    Upload {
        id: String,
        connection_id: String,
        bucket: String,
        files: Vec<ResumeUploadFile>,
        storage_class: Option<String>,
        created_at: u64,
    },
    Download {
        id: String,
        connection_id: String,
        bucket: String,
        items: Vec<ResumeItem>,
        dest: String,
        conflict: String,
        created_at: u64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResumeItem {
    pub key: String,
    pub is_dir: bool,
}

/// 小文件 `upload_id` 为空，恢复时按原文件重新上传；multipart 文件保存检查点。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResumeUploadFile {
    pub path: String,
    pub key: String,
    #[serde(default)]
    pub upload_id: Option<String>,
    #[serde(default)]
    pub part_size: u64,
    #[serde(default)]
    pub completed_parts: Vec<ResumePart>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResumePart {
    pub part_number: i32,
    pub e_tag: String,
}

/// 文件不存在代表无可恢复任务；格式损坏则拒绝加载，避免静默丢任务。
pub fn load_resume_file(path: &Path) -> Result<ResumeFile, CoreError> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(ResumeFile::default()),
        Err(err) => {
            return Err(CoreError::io(
                format!("读取恢复任务失败 {}", path.display()),
                err,
            ))
        }
    };
    serde_json::from_str(&text)
        .map_err(|_| CoreError::Other("恢复任务文件格式无效，未加载历史任务".into()))
}

/// 原子写入；Unix 平台权限与连接配置一致。
pub fn save_resume_file(path: &Path, file: &ResumeFile) -> Result<(), CoreError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            CoreError::io(format!("创建恢复任务目录失败 {}", parent.display()), err)
        })?;
    }
    let text = serde_json::to_string_pretty(file)
        .map_err(|err| CoreError::Other(format!("序列化恢复任务失败：{err}")))?;
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, text)
        .map_err(|err| CoreError::io(format!("写入恢复任务失败 {}", tmp.display()), err))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600));
    }
    fs::rename(&tmp, path)
        .map_err(|err| CoreError::io(format!("保存恢复任务失败 {}", path.display()), err))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("transfers.json");
        assert!(load_resume_file(&path).unwrap().tasks.is_empty());
        let file = ResumeFile {
            version: 1,
            tasks: vec![ResumeTask::Upload {
                id: "task-1".into(),
                connection_id: "connection-1".into(),
                bucket: "bucket".into(),
                files: vec![ResumeUploadFile {
                    path: "/tmp/source".into(),
                    key: "source".into(),
                    upload_id: Some("upload-id".into()),
                    part_size: 8,
                    completed_parts: vec![ResumePart {
                        part_number: 1,
                        e_tag: "etag".into(),
                    }],
                }],
                storage_class: None,
                created_at: 1,
            }],
        };
        save_resume_file(&path, &file).unwrap();
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("upload-id"));
        assert!(!content.contains("secret_key"));
        assert_eq!(load_resume_file(&path).unwrap().tasks.len(), 1);
    }
}
