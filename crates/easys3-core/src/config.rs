//! 连接配置：数据模型、校验、存储与损坏恢复。
//! 规格：`.agents/connection-config.md`。

use crate::error::CoreError;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

fn default_true() -> bool {
    true
}

/// 连接 = 一条 S3 服务配置（扁平列表，无分组层级，规格硬性约束）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ConnectionConfig {
    /// UUID，创建时生成，重命名不变
    pub id: String,
    /// 显示名，非空且全局唯一
    pub name: String,
    /// http/https URL
    pub endpoint_url: String,
    pub region: String,
    pub access_key: String,
    pub secret_key: String,
    /// 默认 true（内部以 S3 兼容服务为主）
    #[serde(default = "default_true")]
    pub force_path_style: bool,
    /// 可选；限定单桶连接（AK 无 ListBuckets 权限时使用）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_bucket: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionsFile {
    pub version: u32,
    #[serde(default)]
    pub connections: Vec<ConnectionConfig>,
    #[serde(default)]
    pub current_connection_id: Option<String>,
}

impl Default for ConnectionsFile {
    fn default() -> Self {
        ConnectionsFile {
            version: 1,
            connections: Vec::new(),
            current_connection_id: None,
        }
    }
}

/// 加载配置；文件不存在返回空配置；JSON 损坏时备份原文件后返回空配置（不得静默清空）。
pub fn load_connections(path: &Path) -> Result<ConnectionsFile, CoreError> {
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(ConnectionsFile::default()),
        Err(e) => return Err(CoreError::io(format!("读取配置失败 {}", path.display()), e)),
    };
    match serde_json::from_str::<ConnectionsFile>(&text) {
        Ok(f) => Ok(f),
        Err(_) => {
            let ts = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let file_name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "connections.json".to_string());
            let backup = path.with_file_name(format!("{file_name}.bak.{ts}"));
            let _ = fs::rename(path, &backup);
            Ok(ConnectionsFile::default())
        }
    }
}

/// 原子写入（先写临时文件再 rename），Unix 下权限 0600（仅当前用户可读）。
pub fn save_connections(path: &Path, file: &ConnectionsFile) -> Result<(), CoreError> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)
            .map_err(|e| CoreError::io(format!("创建配置目录失败 {}", dir.display()), e))?;
    }
    let text = serde_json::to_string_pretty(file)
        .map_err(|e| CoreError::Other(format!("序列化配置失败：{e}")))?;
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, text)
        .map_err(|e| CoreError::io(format!("写入配置失败 {}", tmp.display()), e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600));
    }
    fs::rename(&tmp, path)
        .map_err(|e| CoreError::io(format!("保存配置失败 {}", path.display()), e))?;
    Ok(())
}

/// 校验规则：`.agents/connection-config.md`「校验规则」。others 为现有连接列表（编辑时排除自身）。
pub fn validate_connection(
    p: &ConnectionConfig,
    others: &[ConnectionConfig],
) -> Result<(), Vec<String>> {
    let mut errs: Vec<String> = Vec::new();
    if p.name.trim().is_empty() {
        errs.push("名称不能为空".to_string());
    }
    if others.iter().any(|o| o.id != p.id && o.name == p.name) {
        errs.push(format!("名称“{}”已存在", p.name));
    }
    let ep = p.endpoint_url.trim();
    let rest = ep
        .strip_prefix("http://")
        .or_else(|| ep.strip_prefix("https://"));
    match rest {
        Some(r) if !r.is_empty() && !r.chars().any(char::is_whitespace) => {}
        _ => errs.push("endpoint 必须是合法的 http/https URL".to_string()),
    }
    if p.region.trim().is_empty() {
        errs.push("region 不能为空".to_string());
    }
    if p.access_key.trim().is_empty() {
        errs.push("Access Key 不能为空".to_string());
    }
    if p.secret_key.trim().is_empty() {
        errs.push("Secret Key 不能为空".to_string());
    }
    if let Some(b) = p.default_bucket.as_deref() {
        if !b.is_empty() && !is_valid_bucket_name(b) {
            errs.push(format!(
                "“{b}”不是合法的桶名（3-63 位小写字母/数字/点/连字符）"
            ));
        }
    }
    if errs.is_empty() {
        Ok(())
    } else {
        Err(errs)
    }
}

/// 桶名规范：3-63 位小写字母/数字/点/连字符，首尾为字母或数字。
pub fn is_valid_bucket_name(name: &str) -> bool {
    let n = name.len();
    (3..=63).contains(&n)
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-')
        && name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric())
        && name
            .chars()
            .last()
            .is_some_and(|c| c.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ConnectionConfig {
        ConnectionConfig {
            id: "id-1".to_string(),
            name: "测试服务".to_string(),
            endpoint_url: "https://s3.example.com".to_string(),
            region: "us-east-1".to_string(),
            access_key: "ak".to_string(),
            secret_key: "sk".to_string(),
            force_path_style: true,
            default_bucket: None,
        }
    }

    #[test]
    fn save_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("connections.json");
        let mut file = ConnectionsFile::default();
        file.connections.push(sample());
        file.current_connection_id = Some("id-1".to_string());
        save_connections(&path, &file).unwrap();
        let loaded = load_connections(&path).unwrap();
        assert_eq!(loaded.connections.len(), 1);
        assert_eq!(loaded.connections[0].name, "测试服务");
        assert_eq!(loaded.current_connection_id.as_deref(), Some("id-1"));
    }

    #[test]
    fn missing_file_returns_default() {
        let dir = tempfile::tempdir().unwrap();
        let loaded = load_connections(&dir.path().join("none.json")).unwrap();
        assert!(loaded.connections.is_empty());
    }

    #[test]
    fn corrupted_file_backed_up_and_reset() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("connections.json");
        std::fs::write(&path, "{ not json !!!").unwrap();
        let loaded = load_connections(&path).unwrap();
        assert!(loaded.connections.is_empty());
        // 备份文件已生成
        let bak = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .find(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("connections.json.bak.")
            });
        assert!(bak.is_some(), "损坏的配置必须备份而不是静默清空");
    }

    #[test]
    fn validate_ok() {
        assert!(validate_connection(&sample(), &[]).is_ok());
    }

    #[test]
    fn validate_failures() {
        let mut p = sample();
        p.name = " ".to_string();
        p.endpoint_url = "ftp://x".to_string();
        p.region = String::new();
        p.access_key = String::new();
        p.secret_key = String::new();
        p.default_bucket = Some("Bad_Bucket".to_string());
        let errs = validate_connection(&p, &[]).unwrap_err();
        assert_eq!(errs.len(), 6);

        let mut other = sample();
        other.id = "id-2".to_string();
        assert!(
            validate_connection(&sample(), &[other]).is_err(),
            "名称重复必须报错"
        );
    }

    #[test]
    fn bucket_name_rules() {
        assert!(is_valid_bucket_name("abc"));
        assert!(is_valid_bucket_name("a-b.c9"));
        assert!(!is_valid_bucket_name("ab"));
        assert!(!is_valid_bucket_name("Abc"));
        assert!(!is_valid_bucket_name("-abc"));
        assert!(!is_valid_bucket_name("abc-"));
        assert!(!is_valid_bucket_name("bad_bucket"));
        let too_long = "a".repeat(64);
        assert!(!is_valid_bucket_name(&too_long));
    }

    #[test]
    fn secret_not_in_serialize_output_by_mistake_is_fine_but_roundtrip_keeps_it() {
        // 密钥是本地明文存储基线（规格），但序列化必须无损往返
        let p = sample();
        let json = serde_json::to_string(&p).unwrap();
        let back: ConnectionConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back.secret_key, p.secret_key);
    }
}
