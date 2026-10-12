//! Tauri 命令层：前端 invoke 入口。
//! 错误一律返回中文可读字符串（easys3-core 已保证不含 secret_key）。

use crate::state::{lock, App};
use crate::tasks::{self, FsItem, RetryJob, TaskEntry, TaskInfo, TaskKind, TaskShared, TaskStatus};
use aes_gcm::{
    aead::{Aead, KeyInit, OsRng},
    Aes256Gcm, Nonce,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use easys3_core::config::{connection_fields_changed, validate_connection, ConnectionConfig};
use easys3_core::list::ListResult;
use easys3_core::plan::{collect_upload_files, item_matches_failure};
use easys3_core::preview::PreviewData;
use easys3_core::resume::{ResumePart, ResumeTask, ResumeUploadFile};
use easys3_core::settings::{save_settings, TransferSettings};
use easys3_core::sync::SyncPlan;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use tauri::State;
use uuid::Uuid;

#[derive(Serialize)]
pub struct StateDto {
    pub connections: Vec<ConnectionConfig>,
    pub current_connection_id: Option<String>,
}

/// P1-01：应用重启后由 `transfers.json` 恢复的任务摘要，不含本地路径和凭据。
#[derive(Serialize)]
pub struct ResumableTaskInfo {
    pub id: String,
    pub kind: String,
    pub bucket: String,
    pub created_at: u64,
}

fn transfer_cipher(password: &str, salt: &[u8]) -> Aes256Gcm {
    let mut hash = Sha256::new();
    hash.update(password.as_bytes());
    hash.update(salt);
    Aes256Gcm::new_from_slice(&hash.finalize()).expect("SHA-256 key length")
}

fn encrypt_export(text: &str, password: &str) -> Result<String, String> {
    let mut salt = [0u8; 16];
    let mut nonce = [0u8; 12];
    rand::RngCore::fill_bytes(&mut OsRng, &mut salt);
    rand::RngCore::fill_bytes(&mut OsRng, &mut nonce);
    let encrypted = transfer_cipher(password, &salt)
        .encrypt(Nonce::from_slice(&nonce), text.as_bytes())
        .map_err(|_| "加密导出配置失败".to_string())?;
    Ok(json!({"version": 1, "encrypted": true, "salt": STANDARD.encode(salt), "nonce": STANDARD.encode(nonce), "data": STANDARD.encode(encrypted)}).to_string())
}

fn decrypt_import(text: &str, password: Option<&str>) -> Result<String, String> {
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|_| "导入文件格式无效".to_string())?;
    if value.get("encrypted") != Some(&serde_json::Value::Bool(true)) {
        return Ok(text.to_string());
    }
    let password = password
        .filter(|value| !value.is_empty())
        .ok_or("该配置文件含凭据，请输入导出密码")?;
    let decode = |name: &str| -> Result<Vec<u8>, String> {
        let encoded = value
            .get(name)
            .and_then(|value| value.as_str())
            .ok_or_else(|| "加密配置文件格式无效".to_string())?;
        STANDARD
            .decode(encoded)
            .map_err(|_| "加密配置文件编码无效".to_string())
    };
    let salt = decode("salt")?;
    let nonce = decode("nonce")?;
    let data = decode("data")?;
    if salt.len() != 16 || nonce.len() != 12 {
        return Err("加密配置文件参数无效".to_string());
    }
    let plain = transfer_cipher(password, &salt)
        .decrypt(Nonce::from_slice(&nonce), data.as_ref())
        .map_err(|_| "导出密码错误或配置文件已损坏".to_string())?;
    String::from_utf8(plain).map_err(|_| "加密配置文件内容无效".to_string())
}

#[tauri::command]
pub fn get_state(state: State<App>) -> StateDto {
    let inner = lock(&state.inner);
    let mut connections = inner.file.connections.clone();
    for connection in &mut connections {
        connection.secret_key.clear();
    }
    StateDto {
        connections,
        current_connection_id: inner.file.current_connection_id.clone(),
    }
}

#[tauri::command]
pub fn get_audit(
    state: State<App>,
    connection: Option<String>,
    since: Option<u64>,
) -> Vec<easys3_core::audit::AuditEntry> {
    lock(&state.audit)
        .entries
        .iter()
        .filter(|entry| {
            connection
                .as_ref()
                .is_none_or(|value| &entry.connection == value)
        })
        .filter(|entry| since.is_none_or(|value| entry.at >= value))
        .cloned()
        .collect()
}
#[tauri::command]
pub fn export_audit(
    state: State<App>,
    path: String,
    connection: Option<String>,
    since: Option<u64>,
) -> Result<(), String> {
    let entries = get_audit(state, connection, since);
    std::fs::write(
        path,
        serde_json::to_string_pretty(&entries).map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("导出操作历史失败：{e}"))
}

#[tauri::command]
pub fn generate_iac(bucket: String, format: String) -> Result<String, String> {
    if bucket.trim().is_empty() {
        return Err("桶名不能为空".to_string());
    }
    match format.as_str() {
        "terraform" => Ok(format!("resource \"aws_s3_bucket\" \"{}\" {{\n  bucket = \"{}\"\n}}\n", bucket.replace('-', "_"), bucket)),
        "cloudformation" => Ok(format!("Resources:\n  Bucket:\n    Type: AWS::S3::Bucket\n    Properties:\n      BucketName: {bucket}\n")),
        "cdk" => Ok(format!("new s3.Bucket(this, 'Bucket', {{ bucketName: '{bucket}' }});\n")),
        _ => Err("导出格式仅支持 terraform、cloudformation、cdk".to_string()),
    }
}

#[tauri::command]
pub fn generate_sdk_snippet(
    bucket: String,
    key: String,
    language: String,
) -> Result<String, String> {
    if bucket.trim().is_empty() || key.trim().is_empty() {
        return Err("桶名和对象 Key 不能为空".to_string());
    }
    match language.as_str() {
        "typescript" => Ok(format!("import {{ S3Client, GetObjectCommand }} from '@aws-sdk/client-s3';\nconst client = new S3Client({{ region: 'YOUR_REGION' }});\nconst response = await client.send(new GetObjectCommand({{ Bucket: '{bucket}', Key: '{key}' }}));\n")),
        "python" => Ok(format!("import boto3\ns3 = boto3.client('s3', region_name='YOUR_REGION')\nresponse = s3.get_object(Bucket='{bucket}', Key='{key}')\n")),
        _ => Err("代码语言仅支持 typescript、python".to_string()),
    }
}
#[tauri::command]
pub fn record_audit(
    state: State<App>,
    bucket: String,
    action: String,
    status: String,
    detail: String,
) -> Result<(), String> {
    let connection = state.current_connection()?.name;
    state.record_audit(connection, bucket, action, status, detail);
    Ok(())
}

#[tauri::command]
pub fn open_browser_window(app: tauri::AppHandle) -> Result<(), String> {
    use tauri::{WebviewUrl, WebviewWindowBuilder};
    let label = format!("browser-{}", Uuid::new_v4());
    WebviewWindowBuilder::new(&app, label, WebviewUrl::App("index.html".into()))
        .title("easyS3")
        .inner_size(1280.0, 800.0)
        .min_inner_size(960.0, 600.0)
        .build()
        .map(|_| ())
        .map_err(|e| format!("打开新窗口失败：{e}"))
}

#[tauri::command]
pub fn export_connections(
    state: State<App>,
    path: String,
    secrets: bool,
    password: Option<String>,
) -> Result<(), String> {
    let mut connections = lock(&state.inner).file.connections.clone();
    if secrets {
        for connection in &mut connections {
            connection.secret_key = state.secrets.get(&connection.id)?.unwrap_or_default();
        }
    }
    let text = easys3_core::config::export_connections(&connections, secrets)
        .map_err(|e| e.to_string())?;
    let text = if secrets {
        encrypt_export(
            &text,
            password
                .as_deref()
                .filter(|value| !value.is_empty())
                .ok_or("导出含凭据的配置必须设置密码")?,
        )?
    } else {
        text
    };
    std::fs::write(path, text).map_err(|e| format!("导出连接配置失败：{e}"))
}

#[tauri::command]
pub fn import_connections(
    state: State<App>,
    path: String,
    overwrite: bool,
    password: Option<String>,
) -> Result<usize, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("读取导入文件失败：{e}"))?;
    let text = decrypt_import(&text, password.as_deref())?;
    let incoming = easys3_core::config::import_connections(&text).map_err(|e| e.to_string())?;
    let mut saved = 0;
    {
        let mut inner = lock(&state.inner);
        for mut connection in incoming {
            let position = inner
                .file
                .connections
                .iter()
                .position(|item| item.name == connection.name);
            if position.is_some() && !overwrite {
                continue;
            }
            if connection.id.is_empty() {
                connection.id = Uuid::new_v4().to_string();
            }
            let secret = connection.secret_key.clone();
            connection.secret_key.clear();
            connection.last_test = None;
            if !secret.is_empty() {
                state.secrets.set(&connection.id, &secret)?;
            }
            match position {
                Some(index) => inner.file.connections[index] = connection,
                None => inner.file.connections.push(connection),
            }
            saved += 1;
        }
    }
    state.save_file()?;
    Ok(saved)
}

/// 读取 AWS CLI credentials/config 的 profile；不修改现有连接，交由前端确认后导入。
#[tauri::command]
pub fn import_aws_profiles() -> Result<Vec<ConnectionConfig>, String> {
    let home = std::env::var("HOME").map_err(|_| "无法确定用户主目录".to_string())?;
    let text = std::fs::read_to_string(std::path::Path::new(&home).join(".aws/credentials"))
        .map_err(|e| format!("读取 ~/.aws/credentials 失败：{e}"))?;
    let mut profiles: std::collections::BTreeMap<
        String,
        std::collections::BTreeMap<String, String>,
    > = Default::default();
    let mut section = String::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            section = line[1..line.len() - 1].to_string();
            profiles.entry(section.clone()).or_default();
        } else if let Some((key, value)) = line.split_once('=') {
            if !section.is_empty() {
                profiles
                    .entry(section.clone())
                    .or_default()
                    .insert(key.trim().to_string(), value.trim().to_string());
            }
        }
    }
    // ~/.aws/config 中的 profile 区域覆盖 credentials 内同名 region。
    if let Ok(config) = std::fs::read_to_string(std::path::Path::new(&home).join(".aws/config")) {
        let mut profile = String::new();
        for line in config.lines() {
            let line = line.trim();
            if line.starts_with('[') && line.ends_with(']') {
                profile = line[1..line.len() - 1]
                    .trim_start_matches("profile ")
                    .to_string();
            } else if let Some(("region", value)) = line
                .split_once('=')
                .map(|(key, value)| (key.trim(), value.trim()))
            {
                if !profile.is_empty() {
                    profiles
                        .entry(profile.clone())
                        .or_default()
                        .insert("region".into(), value.into());
                }
            }
        }
    }
    Ok(profiles
        .into_iter()
        .filter_map(|(name, values)| {
            Some(ConnectionConfig {
                id: Uuid::new_v4().to_string(),
                name: format!("AWS {name}"),
                endpoint_url: "https://s3.amazonaws.com".into(),
                region: values
                    .get("region")
                    .cloned()
                    .unwrap_or_else(|| "us-east-1".into()),
                access_key: values.get("aws_access_key_id")?.clone(),
                secret_key: values.get("aws_secret_access_key")?.clone(),
                force_path_style: false,
                read_only: false,
                default_bucket: None,
                ca_cert_path: None,
                last_test: None,
            })
        })
        .collect())
}

#[tauri::command]
pub fn get_resumable_tasks(state: State<App>) -> Vec<ResumableTaskInfo> {
    lock(&state.resume)
        .tasks
        .iter()
        .map(|task| match task {
            ResumeTask::Upload {
                id,
                bucket,
                created_at,
                ..
            } => ResumableTaskInfo {
                id: id.clone(),
                kind: "upload".to_string(),
                bucket: bucket.clone(),
                created_at: *created_at,
            },
            ResumeTask::Download {
                id,
                bucket,
                created_at,
                ..
            } => ResumableTaskInfo {
                id: id.clone(),
                kind: "download".to_string(),
                bucket: bucket.clone(),
                created_at: *created_at,
            },
        })
        .collect()
}

/// 放弃恢复任务只清理本地检查点。上传残片由碎片管理统一提供二次确认清理，
/// 避免在当前连接已切换或删除时对错误的服务端对象发起请求。
#[tauri::command]
pub async fn discard_resumable_task(state: State<'_, App>, id: String) -> Result<(), String> {
    let task = lock(&state.resume)
        .tasks
        .iter()
        .find(|task| match task {
            ResumeTask::Upload { id: task_id, .. } | ResumeTask::Download { id: task_id, .. } => {
                task_id == &id
            }
        })
        .cloned()
        .ok_or("可恢复任务不存在")?;
    if let ResumeTask::Upload {
        connection_id,
        bucket,
        files,
        ..
    } = task
    {
        let connection = state.connection_by_id(&connection_id)?;
        let client = state.client_for_connection(&connection)?;
        for file in files {
            if let Some(upload_id) = file.upload_id.filter(|value| !value.is_empty()) {
                client
                    .abort_multipart_upload()
                    .bucket(&bucket)
                    .key(file.key)
                    .upload_id(upload_id)
                    .send()
                    .await
                    .map_err(|error| easys3_core::error::classify(&error).to_string())?;
            }
        }
    }
    remove_resume_task(&state.resume_path, &state.resume, &id);
    Ok(())
}

/// 将重启前的检查点恢复为实际后台任务。恢复始终使用任务原连接，避免用户切换连接后
/// 意外传输到另一个服务。
#[tauri::command]
pub fn resume_task(state: State<App>, app: tauri::AppHandle, id: String) -> Result<(), String> {
    let task = lock(&state.resume)
        .tasks
        .iter()
        .find(|task| match task {
            ResumeTask::Upload { id: task_id, .. } | ResumeTask::Download { id: task_id, .. } => {
                task_id == &id
            }
        })
        .cloned()
        .ok_or("可恢复任务不存在")?;

    match task {
        ResumeTask::Upload {
            id,
            connection_id,
            bucket,
            files,
            storage_class,
            created_at,
        } => {
            let connection = state.connection_by_id(&connection_id)?;
            let client = state.client_for_connection(&connection)?;
            let upload_files: Vec<_> = files
                .iter()
                .map(|file| (PathBuf::from(&file.path), file.key.clone()))
                .collect();
            if upload_files.is_empty() {
                return Err("恢复任务没有可上传文件".to_string());
            }
            let bytes_total = tasks::upload_files_size(&upload_files);
            let cancel = Arc::new(AtomicBool::new(false));
            let shared = TaskShared::new(tasks::new_task_info(
                &id,
                TaskKind::Upload,
                &bucket,
                upload_files.len(),
                bytes_total,
            ));
            let checkpoints = Arc::new(
                files
                    .iter()
                    .map(|file| {
                        (
                            file.key.clone(),
                            easys3_core::upload::MultipartCheckpoint {
                                upload_id: file.upload_id.clone(),
                                part_size: file.part_size,
                                completed_parts: file
                                    .completed_parts
                                    .iter()
                                    .map(|part| (part.part_number, part.e_tag.clone()))
                                    .collect(),
                            },
                        )
                    })
                    .collect(),
            );
            let retry = Arc::new(Mutex::new(None));
            {
                let mut inner = lock(&state.inner);
                if inner.tasks.contains_key(&id) {
                    return Err("任务正在进行中".to_string());
                }
                inner.tasks.insert(
                    id.clone(),
                    TaskEntry {
                        cancel: cancel.clone(),
                        shared: shared.clone(),
                        retry,
                    },
                );
            }
            let storage_class = storage_class
                .as_deref()
                .map(str::parse)
                .transpose()
                .map_err(|_| "不支持的存储类别".to_string())?;
            let resume = state.resume.clone();
            let resume_path = state.resume_path.clone();
            let callback_id = id.clone();
            let changed = Arc::new(
                move |key: &str, checkpoint: easys3_core::upload::MultipartCheckpoint| {
                    {
                        let mut file = lock(&resume);
                        if let Some(ResumeTask::Upload { files, .. }) = file.tasks.iter_mut().find(|task| matches!(task, ResumeTask::Upload { id, .. } if id == &callback_id)) {
                        if let Some(item) = files.iter_mut().find(|item| item.key == key) {
                            item.upload_id = checkpoint.upload_id;
                            item.part_size = checkpoint.part_size;
                            item.completed_parts = checkpoint.completed_parts.into_iter().map(|(part_number, e_tag)| ResumePart { part_number, e_tag }).collect();
                        }
                    }
                    }
                    save_resume_snapshot(&resume_path, &resume);
                },
            );
            let resume = state.resume.clone();
            let resume_path = state.resume_path.clone();
            let callback_id = id.clone();
            let completed =
                Arc::new(move || remove_resume_task(&resume_path, &resume, &callback_id));
            tasks::spawn_upload(
                app,
                client,
                bucket,
                upload_files,
                cancel,
                shared,
                storage_class,
                state.transfer_settings(),
                state.upload_throttle.clone(),
                false,
                Some(changed),
                Some(checkpoints),
                Some(completed),
            );
            let _ = created_at;
        }
        ResumeTask::Download {
            id,
            connection_id,
            bucket,
            items,
            dest,
            conflict,
            created_at,
        } => {
            let connection = state.connection_by_id(&connection_id)?;
            let client = state.client_for_connection(&connection)?;
            let control = easys3_core::s3::build_control_client(&connection)
                .map_err(|error| error.to_string())?;
            let items: Vec<FsItem> = items
                .into_iter()
                .map(|item| FsItem {
                    key: item.key,
                    is_dir: item.is_dir,
                })
                .collect();
            if !std::path::Path::new(&dest).is_dir() {
                return Err("原下载目录不存在，无法恢复任务".to_string());
            }
            let cancel = Arc::new(AtomicBool::new(false));
            let shared = TaskShared::new(tasks::new_task_info(
                &id,
                TaskKind::Download,
                &bucket,
                items.len(),
                0,
            ));
            let retry = Arc::new(Mutex::new(None));
            {
                let mut inner = lock(&state.inner);
                if inner.tasks.contains_key(&id) {
                    return Err("任务正在进行中".to_string());
                }
                inner.tasks.insert(
                    id.clone(),
                    TaskEntry {
                        cancel: cancel.clone(),
                        shared: shared.clone(),
                        retry: retry.clone(),
                    },
                );
            }
            let resume = state.resume.clone();
            let resume_path = state.resume_path.clone();
            let callback_id = id.clone();
            let completed =
                Arc::new(move || remove_resume_task(&resume_path, &resume, &callback_id));
            tasks::spawn_download_expand(
                app,
                client,
                control,
                bucket,
                items,
                PathBuf::from(dest),
                conflict,
                None,
                cancel,
                shared,
                retry,
                state.transfer_settings(),
                state.download_throttle.clone(),
                true,
                Some(completed),
            );
            let _ = created_at;
        }
    }
    Ok(())
}

fn save_resume_snapshot(
    path: &std::path::Path,
    resume: &Arc<Mutex<easys3_core::resume::ResumeFile>>,
) {
    let file = lock(resume).clone();
    let _ = easys3_core::resume::save_resume_file(path, &file);
}

fn remove_resume_task(
    path: &std::path::Path,
    resume: &Arc<Mutex<easys3_core::resume::ResumeFile>>,
    id: &str,
) {
    lock(resume).tasks.retain(|task| match task {
        ResumeTask::Upload { id: task_id, .. } | ResumeTask::Download { id: task_id, .. } => {
            task_id != id
        }
    });
    save_resume_snapshot(path, resume);
}

#[tauri::command]
pub fn save_connection(state: State<App>, connection: ConnectionConfig) -> Result<(), String> {
    let connection_id = connection.id.clone();
    let supplied_secret = connection.secret_key.trim().to_string();
    let existing_secret = {
        let inner = lock(&state.inner);
        inner
            .file
            .connections
            .iter()
            .find(|item| item.id == connection.id)
            .and_then(|_| state.secrets.get(&connection.id).ok().flatten())
    };
    let mut connection = connection;
    if supplied_secret.is_empty() {
        connection.secret_key = existing_secret.clone().unwrap_or_default();
    }
    if connection.secret_key.is_empty() {
        return Err("Secret Key 不能为空".to_string());
    }
    // 先持久化密钥；失败时不改变连接配置或客户端缓存。
    if !supplied_secret.is_empty() {
        state.secrets.set(&connection_id, &supplied_secret)?;
    }
    {
        let mut inner = lock(&state.inner);
        if let Err(errs) = validate_connection(&connection, &inner.file.connections) {
            return Err(errs.join("；"));
        }
        match inner
            .file
            .connections
            .iter_mut()
            .find(|p| p.id == connection.id)
        {
            Some(existing) => {
                let mut updated = connection.clone();
                let mut old_for_compare = existing.clone();
                old_for_compare.secret_key = existing_secret.unwrap_or_default();
                // 连接相关字段变更后，已持久化的测试结果作废清空（规格）
                if connection_fields_changed(&old_for_compare, &updated) {
                    updated.last_test = None;
                }
                updated.secret_key.clear();
                *existing = updated;
            }
            None => {
                let mut saved = connection.clone();
                saved.secret_key.clear();
                inner.file.connections.push(saved);
            }
        }
        // 配置变更后重建该连接的 S3 客户端（传输 + 控制面两类缓存）
        inner.clients.remove(&connection.id);
        inner.control_clients.remove(&connection.id);
        if inner.file.current_connection_id.is_none() {
            inner.file.current_connection_id = Some(connection.id);
        }
    }
    state.save_file()
}

#[tauri::command]
pub fn delete_connection(state: State<App>, id: String) -> Result<(), String> {
    {
        let mut inner = lock(&state.inner);
        inner.file.connections.retain(|p| p.id != id);
        inner.clients.remove(&id);
        inner.control_clients.remove(&id);
        // 删除当前连接：自动选中剩余第一个，无连接则为空（规格）
        if inner.file.current_connection_id.as_deref() == Some(id.as_str()) {
            inner.file.current_connection_id = inner.file.connections.first().map(|p| p.id.clone());
        }
    }
    state.secrets.delete(&id);
    state.save_file()
}

#[tauri::command]
pub fn set_current_connection(state: State<App>, id: String) -> Result<(), String> {
    {
        let mut inner = lock(&state.inner);
        if !inner.file.connections.iter().any(|p| p.id == id) {
            return Err("连接不存在".to_string());
        }
        inner.file.current_connection_id = Some(id);
    }
    state.save_file()
}

#[tauri::command]
pub async fn test_connection(
    state: State<'_, App>,
    mut connection: ConnectionConfig,
) -> Result<(), String> {
    if connection.secret_key.trim().is_empty() {
        connection.secret_key = state
            .secrets
            .get(&connection.id)?
            .ok_or("未找到该连接的 Secret Key，请在连接设置中重新填写")?;
    }
    easys3_core::s3::test_connection(&connection)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_buckets(state: State<'_, App>) -> Result<Vec<String>, String> {
    let (client, _) = state.current_control_client()?;
    easys3_core::list::list_buckets(&client)
        .await
        .map_err(|e| e.to_string())
}

fn reject_limited_connection(state: &App) -> Result<(), String> {
    if state.current_connection()?.default_bucket.is_some() {
        Err("限定单桶连接不支持此桶管理操作".to_string())
    } else {
        Ok(())
    }
}
fn require_writable(state: &App) -> Result<(), String> {
    if state.current_connection()?.read_only {
        Err("当前连接处于只读模式，不能执行写操作".to_string())
    } else {
        Ok(())
    }
}
#[tauri::command]
pub async fn create_bucket(state: State<'_, App>, bucket: String) -> Result<(), String> {
    require_writable(&state)?;
    reject_limited_connection(&state)?;
    let (client, _) = state.current_control_client()?;
    easys3_core::object::create_bucket(&client, &bucket)
        .await
        .map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn delete_bucket(state: State<'_, App>, bucket: String) -> Result<(), String> {
    require_writable(&state)?;
    reject_limited_connection(&state)?;
    let (client, _) = state.current_control_client()?;
    easys3_core::object::delete_bucket(&client, &bucket)
        .await
        .map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn bucket_versioning(
    state: State<'_, App>,
    bucket: String,
) -> Result<easys3_core::object::BucketVersioningInfo, String> {
    reject_limited_connection(&state)?;
    let (client, _) = state.current_control_client()?;
    easys3_core::object::bucket_versioning(&client, &bucket)
        .await
        .map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn set_bucket_versioning(
    state: State<'_, App>,
    bucket: String,
    enabled: bool,
) -> Result<(), String> {
    require_writable(&state)?;
    reject_limited_connection(&state)?;
    let (client, _) = state.current_control_client()?;
    easys3_core::object::set_bucket_versioning(&client, &bucket, enabled)
        .await
        .map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn bucket_policy(state: State<'_, App>, bucket: String) -> Result<String, String> {
    require_writable(&state)?;
    reject_limited_connection(&state)?;
    let (client, _) = state.current_control_client()?;
    easys3_core::object::get_bucket_policy(&client, &bucket)
        .await
        .map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn save_bucket_policy(
    state: State<'_, App>,
    bucket: String,
    policy: String,
) -> Result<(), String> {
    require_writable(&state)?;
    reject_limited_connection(&state)?;
    let (client, _) = state.current_client()?;
    easys3_core::object::put_bucket_policy(&client, &bucket, &policy)
        .await
        .map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn bucket_cors(
    state: State<'_, App>,
    bucket: String,
) -> Result<Vec<easys3_core::object::CorsRuleDto>, String> {
    require_writable(&state)?;
    reject_limited_connection(&state)?;
    let (client, _) = state.current_control_client()?;
    easys3_core::object::get_bucket_cors(&client, &bucket)
        .await
        .map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn save_bucket_cors(
    state: State<'_, App>,
    bucket: String,
    rules: Vec<easys3_core::object::CorsRuleDto>,
) -> Result<(), String> {
    require_writable(&state)?;
    reject_limited_connection(&state)?;
    let (client, _) = state.current_client()?;
    easys3_core::object::put_bucket_cors(&client, &bucket, rules)
        .await
        .map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn bucket_lifecycle(
    state: State<'_, App>,
    bucket: String,
) -> Result<Vec<easys3_core::object::LifecycleRuleDto>, String> {
    require_writable(&state)?;
    reject_limited_connection(&state)?;
    let (client, _) = state.current_control_client()?;
    easys3_core::object::get_bucket_lifecycle(&client, &bucket)
        .await
        .map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn save_bucket_lifecycle(
    state: State<'_, App>,
    bucket: String,
    rules: Vec<easys3_core::object::LifecycleRuleDto>,
) -> Result<(), String> {
    require_writable(&state)?;
    reject_limited_connection(&state)?;
    let (client, _) = state.current_client()?;
    easys3_core::object::put_bucket_lifecycle(&client, &bucket, rules)
        .await
        .map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn bucket_website(
    state: State<'_, App>,
    bucket: String,
) -> Result<easys3_core::object::WebsiteDto, String> {
    require_writable(&state)?;
    reject_limited_connection(&state)?;
    let (client, _) = state.current_control_client()?;
    easys3_core::object::get_bucket_website(&client, &bucket)
        .await
        .map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn save_bucket_website(
    state: State<'_, App>,
    bucket: String,
    website: easys3_core::object::WebsiteDto,
) -> Result<(), String> {
    require_writable(&state)?;
    reject_limited_connection(&state)?;
    let (client, _) = state.current_client()?;
    easys3_core::object::put_bucket_website(&client, &bucket, website)
        .await
        .map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn object_lock(
    state: State<'_, App>,
    bucket: String,
) -> Result<easys3_core::object::ObjectLockDto, String> {
    reject_limited_connection(&state)?;
    let (client, _) = state.current_control_client()?;
    easys3_core::object::get_object_lock(&client, &bucket)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_objects(
    state: State<'_, App>,
    bucket: String,
    prefix: String,
    token: Option<String>,
) -> Result<ListResult, String> {
    let (client, _) = state.current_control_client()?;
    easys3_core::list::list_objects(&client, &bucket, &prefix, token.as_deref())
        .await
        .map_err(|e| e.to_string())
}

/// 删除前缀前的对象计数（规格：确认框必须显示实际数量；含随前缀一并删除的目录占位对象）
#[tauri::command]
pub async fn count_objects(
    state: State<'_, App>,
    bucket: String,
    prefix: String,
) -> Result<u64, String> {
    let (client, _) = state.current_control_client()?;
    let cancel = Arc::new(AtomicBool::new(false));
    // true = 统计含目录占位对象（与实际删除行为一致，规格 §4）
    let keys = easys3_core::list::collect_prefix_objects(&client, &bucket, &prefix, &cancel, true)
        .await
        .map_err(|e| e.to_string())?;
    Ok(keys.len() as u64)
}

#[tauri::command]
pub async fn prefix_usage(
    state: State<'_, App>,
    bucket: String,
    prefix: String,
) -> Result<easys3_core::list::UsageStats, String> {
    let (client, _) = state.current_control_client()?;
    let cancel = Arc::new(AtomicBool::new(false));
    easys3_core::list::count_prefix_usage(&client, &bucket, &prefix, &cancel)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn start_prefix_usage(
    state: State<App>,
    app: tauri::AppHandle,
    bucket: String,
    prefix: String,
) -> Result<String, String> {
    let (client, _) = state.current_control_client()?;
    let id = Uuid::new_v4().to_string();
    let cancel = Arc::new(AtomicBool::new(false));
    let shared = TaskShared::new(tasks::new_task_info(&id, TaskKind::Stats, &bucket, 0, 0));
    lock(&state.inner).tasks.insert(
        id.clone(),
        TaskEntry {
            cancel: cancel.clone(),
            shared: shared.clone(),
            retry: Arc::new(Mutex::new(None)),
        },
    );
    tasks::spawn_usage_stats(app, client, id.clone(), bucket, prefix, cancel, shared);
    Ok(id)
}

#[tauri::command]
pub async fn plan_sync_upload(
    state: State<'_, App>,
    bucket: String,
    prefix: String,
    local: String,
    propagate: bool,
) -> Result<easys3_core::sync::SyncPlan, String> {
    let path = PathBuf::from(&local);
    if !path.is_dir() {
        return Err("本地同步目录不存在".to_string());
    }
    let (client, _) = state.current_control_client()?;
    let cancel = Arc::new(AtomicBool::new(false));
    let remote = easys3_core::list::collect_prefix_sizes(&client, &bucket, &prefix, &cancel)
        .await
        .map_err(|e| e.to_string())?;
    easys3_core::sync::plan_upload(&path, &prefix, &remote, propagate)
        .map_err(|e| format!("生成同步计划失败：{e}"))
}

#[tauri::command]
pub async fn plan_sync_download(
    state: State<'_, App>,
    bucket: String,
    prefix: String,
    local: String,
    propagate: bool,
) -> Result<easys3_core::sync::SyncPlan, String> {
    let path = PathBuf::from(&local);
    if !path.is_dir() {
        return Err("本地同步目录不存在".to_string());
    }
    let (client, _) = state.current_control_client()?;
    let cancel = Arc::new(AtomicBool::new(false));
    let remote = easys3_core::list::collect_prefix_sizes(&client, &bucket, &prefix, &cancel)
        .await
        .map_err(|e| e.to_string())?;
    easys3_core::sync::plan_download(&path, &prefix, &remote, propagate)
        .map_err(|e| format!("生成同步计划失败：{e}"))
}

#[tauri::command]
pub async fn start_search(
    state: State<'_, App>,
    app: tauri::AppHandle,
    bucket: Option<String>,
    query: String,
) -> Result<String, String> {
    let filter = easys3_core::search::parse(&query).map_err(|e| e.to_string())?;
    let (client, _) = state.current_control_client()?;
    let buckets = match bucket.filter(|bucket| !bucket.is_empty()) {
        Some(bucket) => vec![bucket],
        None => easys3_core::list::list_buckets(&client)
            .await
            .map_err(|e| e.to_string())?,
    };
    let id = Uuid::new_v4().to_string();
    let cancel = Arc::new(AtomicBool::new(false));
    let shared = TaskShared::new(tasks::new_task_info(
        &id,
        TaskKind::Search,
        "搜索",
        buckets.len(),
        0,
    ));
    lock(&state.inner).tasks.insert(
        id.clone(),
        TaskEntry {
            cancel: cancel.clone(),
            shared: shared.clone(),
            retry: Arc::new(Mutex::new(None)),
        },
    );
    tasks::spawn_search(app, client, id.clone(), buckets, filter, cancel, shared);
    Ok(id)
}

#[tauri::command]
pub fn start_sync_upload(
    state: State<App>,
    app: tauri::AppHandle,
    bucket: String,
    plan: SyncPlan,
) -> Result<Vec<String>, String> {
    let files: Vec<(PathBuf, String)> = plan
        .create
        .iter()
        .chain(plan.update.iter())
        .map(|item| (PathBuf::from(&item.source), item.target.clone()))
        .collect();
    if files.is_empty() && plan.delete.is_empty() {
        return Err("同步计划没有需要上传的文件".to_string());
    }
    if files.is_empty() {
        let items = plan
            .delete
            .into_iter()
            .map(|item| FsItem {
                key: item.source,
                is_dir: false,
            })
            .collect();
        return Ok(vec![start_delete(state, app, bucket, items)?]);
    }
    let (client, _) = state.current_client()?;
    let id = Uuid::new_v4().to_string();
    let cancel = Arc::new(AtomicBool::new(false));
    let shared = TaskShared::new(tasks::new_task_info(
        &id,
        TaskKind::Upload,
        &bucket,
        files.len(),
        tasks::upload_files_size(&files),
    ));
    let retry = Arc::new(Mutex::new(Some(RetryJob::Upload {
        client: client.clone(),
        bucket: bucket.clone(),
        files: files.clone(),
    })));
    lock(&state.inner).tasks.insert(
        id.clone(),
        TaskEntry {
            cancel: cancel.clone(),
            shared: shared.clone(),
            retry,
        },
    );
    tasks::spawn_upload(
        app.clone(),
        client,
        bucket.clone(),
        files,
        cancel,
        shared,
        None,
        state.transfer_settings(),
        state.upload_throttle.clone(),
        false,
        None,
        None,
        None,
    );
    // 删除传播作为独立 Delete 任务，沿用已有取消与精确重试语义。
    let mut ids = vec![id];
    if !plan.delete.is_empty() {
        let items = plan
            .delete
            .into_iter()
            .map(|item| FsItem {
                key: item.source,
                is_dir: false,
            })
            .collect();
        ids.push(start_delete(state, app, bucket, items)?);
    }
    Ok(ids)
}

#[tauri::command]
pub fn start_sync_download(
    state: State<App>,
    app: tauri::AppHandle,
    bucket: String,
    plan: SyncPlan,
) -> Result<Vec<String>, String> {
    let jobs: Vec<tasks::DownloadJob> = plan
        .create
        .iter()
        .chain(plan.update.iter())
        .map(|item| tasks::DownloadJob {
            key: item.source.clone(),
            dest: PathBuf::from(&item.target),
        })
        .collect();
    if jobs.is_empty() && plan.delete.is_empty() {
        return Err("同步计划没有需要下载的文件".to_string());
    }
    if jobs.is_empty() {
        return start_sync_local_deletes(state, app, bucket, plan.delete);
    }
    let (client, _) = state.current_client()?;
    let id = Uuid::new_v4().to_string();
    let cancel = Arc::new(AtomicBool::new(false));
    let shared = TaskShared::new(tasks::new_task_info(
        &id,
        TaskKind::Download,
        &bucket,
        jobs.len(),
        0,
    ));
    let retry = Arc::new(Mutex::new(Some(RetryJob::SyncDownload {
        client: client.clone(),
        bucket: bucket.clone(),
        jobs: jobs.clone(),
    })));
    lock(&state.inner).tasks.insert(
        id.clone(),
        TaskEntry {
            cancel: cancel.clone(),
            shared: shared.clone(),
            retry,
        },
    );
    tasks::spawn_sync_download(
        app.clone(),
        client,
        bucket.clone(),
        jobs,
        cancel,
        shared,
        state.transfer_settings(),
        state.download_throttle.clone(),
    );
    let mut ids = vec![id];
    if !plan.delete.is_empty() {
        ids.extend(start_sync_local_deletes(state, app, bucket, plan.delete)?);
    }
    Ok(ids)
}

#[tauri::command]
pub fn start_zip_download(
    state: State<App>,
    app: tauri::AppHandle,
    bucket: String,
    items: Vec<FsItem>,
    dest: String,
) -> Result<String, String> {
    if items.is_empty() {
        return Err("请选择要打包的文件或文件夹".to_string());
    }
    let (client, _) = state.current_client()?;
    let (control, _) = state.current_control_client()?;
    let id = Uuid::new_v4().to_string();
    let cancel = Arc::new(AtomicBool::new(false));
    let shared = TaskShared::new(tasks::new_task_info(
        &id,
        TaskKind::Zip,
        &bucket,
        items.len(),
        0,
    ));
    lock(&state.inner).tasks.insert(
        id.clone(),
        TaskEntry {
            cancel: cancel.clone(),
            shared: shared.clone(),
            retry: Arc::new(Mutex::new(None)),
        },
    );
    tasks::spawn_zip(
        app,
        client,
        control,
        bucket,
        items,
        PathBuf::from(dest),
        cancel,
        shared,
        state.download_throttle.clone(),
    );
    Ok(id)
}

fn start_sync_local_deletes(
    state: State<App>,
    app: tauri::AppHandle,
    bucket: String,
    items: Vec<easys3_core::sync::SyncPlanItem>,
) -> Result<Vec<String>, String> {
    let paths = items
        .into_iter()
        .map(|item| PathBuf::from(item.target))
        .collect::<Vec<_>>();
    let id = Uuid::new_v4().to_string();
    let cancel = Arc::new(AtomicBool::new(false));
    let shared = TaskShared::new(tasks::new_task_info(
        &id,
        TaskKind::Delete,
        &bucket,
        paths.len(),
        0,
    ));
    lock(&state.inner).tasks.insert(
        id.clone(),
        TaskEntry {
            cancel: cancel.clone(),
            shared: shared.clone(),
            retry: Arc::new(Mutex::new(Some(RetryJob::SyncLocalDelete {
                bucket: bucket.clone(),
                paths: paths.clone(),
            }))),
        },
    );
    tasks::spawn_sync_local_delete(app, paths, cancel, shared);
    Ok(vec![id])
}

#[tauri::command]
pub async fn preview_object(
    state: State<'_, App>,
    bucket: String,
    key: String,
) -> Result<PreviewData, String> {
    let (client, _) = state.current_control_client()?;
    let media_limit = state.transfer_settings().preview_limit_bytes();
    easys3_core::preview::preview_object(&client, &bucket, &key, media_limit)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn thumbnail_object(
    state: State<'_, App>,
    bucket: String,
    key: String,
) -> Result<PreviewData, String> {
    let (client, _) = state.current_control_client()?;
    easys3_core::preview::thumbnail_object(&client, &bucket, &key)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn is_download_directory(dest: String) -> bool {
    std::path::Path::new(&dest).is_dir()
}

#[tauri::command]
pub async fn presign_get(
    state: State<'_, App>,
    bucket: String,
    keys: Vec<String>,
    seconds: u64,
) -> Result<Vec<String>, String> {
    if keys.is_empty() {
        return Err("请选择至少一个对象".to_string());
    }
    let (client, _) = state.current_client()?;
    let mut urls = Vec::with_capacity(keys.len());
    for key in keys {
        urls.push(
            easys3_core::presign::get_url(&client, &bucket, &key, seconds)
                .await
                .map_err(|e| e.to_string())?,
        );
    }
    Ok(urls)
}

#[tauri::command]
pub async fn presign_put(
    state: State<'_, App>,
    bucket: String,
    key: String,
    seconds: u64,
) -> Result<String, String> {
    if key.trim().is_empty() {
        return Err("上传对象名称不能为空".to_string());
    }
    let (client, _) = state.current_client()?;
    easys3_core::presign::put_url(&client, &bucket, &key, seconds)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn export_presign_list(path: String, urls: Vec<String>) -> Result<(), String> {
    if urls.is_empty() {
        return Err("没有可导出的链接".to_string());
    }
    std::fs::write(&path, urls.join("\n")).map_err(|error| format!("导出链接清单失败：{error}"))
}

// ---------- 传输设置（自动重试 / 限速 / 并发 / 分片） ----------

/// "打开"（P0-10）：下载对象到系统临时目录，完成后用默认程序打开。
/// 进度经任务中心反馈、不阻塞界面；临时目录在应用退出时清理。返回任务 id。
#[tauri::command]
pub fn open_object(
    state: State<App>,
    app: tauri::AppHandle,
    bucket: String,
    key: String,
) -> Result<String, String> {
    let (client, _) = state.current_client()?;
    // 仅取 key 末段作为文件名，剥离路径分隔符，避免目录穿越
    let raw = key.trim_end_matches('/').rsplit('/').next().unwrap_or("");
    let basename: String = raw
        .chars()
        .map(|c| {
            if c == '/' || c == '\\' || c == '\0' {
                '_'
            } else {
                c
            }
        })
        .collect();
    let basename = if basename.is_empty() {
        "object".to_string()
    } else {
        basename
    };
    let dest = crate::state::open_temp_dir().join(basename);
    let id = Uuid::new_v4().to_string();
    let cancel = Arc::new(AtomicBool::new(false));
    let shared = TaskShared::new(tasks::new_task_info(&id, TaskKind::Download, &bucket, 1, 0));
    {
        let mut inner = lock(&state.inner);
        inner.tasks.insert(
            id.clone(),
            TaskEntry {
                cancel: cancel.clone(),
                shared: shared.clone(),
                retry: Arc::new(Mutex::new(None)),
            },
        );
    }
    let settings = state.transfer_settings();
    tasks::spawn_open(
        app,
        client,
        bucket,
        key,
        dest,
        cancel,
        shared,
        settings.auto_retry_count as usize,
        state.download_throttle.clone(),
    );
    Ok(id)
}

#[tauri::command]
pub fn get_settings(state: State<App>) -> TransferSettings {
    state.transfer_settings()
}

/// 保存设置：先热更新限速与并发上限（进行中任务即时生效），再落盘。
#[tauri::command]
pub fn save_transfer_settings(state: State<App>, settings: TransferSettings) -> Result<(), String> {
    let settings = settings.sanitized();
    state
        .upload_throttle
        .set_rate_bps(settings.upload_limit_kbps.saturating_mul(1024));
    state
        .download_throttle
        .set_rate_bps(settings.download_limit_kbps.saturating_mul(1024));
    tasks::set_max_concurrent_tasks(settings.max_concurrent_tasks);
    save_settings(&state.settings_path, &settings)?;
    *lock(&state.settings) = settings;
    Ok(())
}

#[tauri::command]
pub async fn object_detail(
    state: State<'_, App>,
    bucket: String,
    key: String,
) -> Result<easys3_core::object::ObjectDetail, String> {
    let (client, _) = state.current_control_client()?;
    easys3_core::object::head_object(&client, &bucket, &key)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn restore_archived_object(
    state: State<'_, App>,
    bucket: String,
    key: String,
    days: i32,
    tier: String,
) -> Result<(), String> {
    require_writable(&state)?;
    let (client, _) = state.current_client()?;
    easys3_core::object::restore_archived_object(&client, &bucket, &key, days, &tier)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn object_versions(
    state: State<'_, App>,
    bucket: String,
    key: String,
) -> Result<Vec<easys3_core::object::ObjectVersionInfo>, String> {
    let (client, _) = state.current_control_client()?;
    easys3_core::object::list_object_versions(&client, &bucket, &key)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn object_tags(
    state: State<'_, App>,
    bucket: String,
    key: String,
) -> Result<Vec<easys3_core::object::ObjectTag>, String> {
    let (client, _) = state.current_control_client()?;
    easys3_core::object::get_object_tags(&client, &bucket, &key)
        .await
        .map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn save_object_tags(
    state: State<'_, App>,
    bucket: String,
    key: String,
    tags: Vec<easys3_core::object::ObjectTag>,
) -> Result<(), String> {
    require_writable(&state)?;
    let (client, _) = state.current_client()?;
    easys3_core::object::put_object_tags(&client, &bucket, &key, &tags)
        .await
        .map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn object_acl(
    state: State<'_, App>,
    bucket: String,
    key: String,
) -> Result<easys3_core::object::ObjectAclInfo, String> {
    let (client, _) = state.current_control_client()?;
    easys3_core::object::get_object_acl(&client, &bucket, &key)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn restore_object_version(
    state: State<'_, App>,
    bucket: String,
    key: String,
    version: String,
) -> Result<(), String> {
    require_writable(&state)?;
    let (client, _) = state.current_client()?;
    easys3_core::object::restore_object_version(&client, &bucket, &key, &version)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn replace_object_metadata(
    state: State<'_, App>,
    bucket: String,
    key: String,
    content_type: String,
    metadata: Vec<easys3_core::object::ObjectTag>,
) -> Result<(), String> {
    require_writable(&state)?;
    let (client, _) = state.current_client()?;
    let pairs = metadata
        .into_iter()
        .map(|item| (item.key, item.value))
        .collect::<Vec<_>>();
    easys3_core::object::replace_object_metadata(&client, &bucket, &key, &content_type, &pairs)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn save_text_object(
    state: State<'_, App>,
    bucket: String,
    key: String,
    content: String,
) -> Result<(), String> {
    require_writable(&state)?;
    let (client, _) = state.current_client()?;
    easys3_core::object::put_text_object(&client, &bucket, &key, content)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn create_folder(
    state: State<'_, App>,
    bucket: String,
    prefix: String,
    name: String,
) -> Result<(), String> {
    require_writable(&state)?;
    let (client, _) = state.current_control_client()?;
    easys3_core::object::create_folder(&client, &bucket, &prefix, &name)
        .await
        .map_err(|e| e.to_string())
}

/// 桶内复制 / 移动 / 重命名（任务化，规格 §6）：服务端 copy_object，不下载中转；
/// 目录递归展开（含 0 字节占位对象）；冲突策略 overwrite / skip；>5GB 明确报错。
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn start_copy(
    state: State<App>,
    app: tauri::AppHandle,
    bucket: String,
    items: Vec<FsItem>,
    target_prefix: String,
    target_bucket: String,
    conflict: String,
    new_name: Option<String>,
    remove: bool,
) -> Result<String, String> {
    require_writable(&state)?;
    if let Some(name) = new_name.as_deref() {
        if name.is_empty() || name.contains('/') {
            return Err("新名称不能为空且不能包含 /".to_string());
        }
    }
    if target_bucket.trim().is_empty() {
        return Err("目标桶不能为空".to_string());
    }
    if !target_prefix.is_empty() && !target_prefix.ends_with('/') {
        return Err("目标前缀必须以 / 结尾（或为空表示桶根）".to_string());
    }
    // 传输客户端：目录展开 + copy 均可能涉及大量请求，不设 30s 超时
    let (client, _) = state.current_client()?;
    let id = Uuid::new_v4().to_string();
    let cancel = Arc::new(AtomicBool::new(false));
    let shared = TaskShared::new(tasks::new_task_info(
        &id,
        TaskKind::Copy,
        &bucket,
        items.len(),
        0,
    ));
    let retry = Arc::new(Mutex::new(None));
    {
        let mut inner = lock(&state.inner);
        inner.tasks.insert(
            id.clone(),
            TaskEntry {
                cancel: cancel.clone(),
                shared: shared.clone(),
                retry: retry.clone(),
            },
        );
    }
    tasks::spawn_copy(
        app,
        client,
        bucket,
        target_bucket,
        items,
        target_prefix,
        conflict,
        new_name,
        remove,
        None,
        cancel,
        shared,
        retry,
        state.transfer_settings(),
    );
    Ok(id)
}

#[tauri::command]
pub async fn list_multipart_uploads(
    state: State<'_, App>,
    bucket: String,
) -> Result<Vec<easys3_core::object::MultipartUploadInfo>, String> {
    let (client, _) = state.current_control_client()?;
    easys3_core::object::list_multipart_uploads(&client, &bucket)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn abort_multipart_uploads(
    state: State<'_, App>,
    bucket: String,
    uploads: Vec<easys3_core::object::MultipartUploadInfo>,
) -> Result<Vec<easys3_core::object::MultipartUploadInfo>, String> {
    let (client, _) = state.current_control_client()?;
    Ok(easys3_core::object::abort_multipart_uploads(&client, &bucket, &uploads).await)
}

#[tauri::command]
pub async fn list_multipart_parts(
    state: State<'_, App>,
    bucket: String,
    key: String,
    upload_id: String,
) -> Result<Vec<easys3_core::object::MultipartPartInfo>, String> {
    let (client, _) = state.current_control_client()?;
    easys3_core::object::list_multipart_parts(&client, &bucket, &key, &upload_id)
        .await
        .map_err(|e| e.to_string())
}

#[derive(Serialize, Deserialize)]
pub struct PlannedFile {
    pub path: String,
    pub key: String,
    pub size: u64,
}

#[derive(Serialize, Deserialize)]
pub struct UploadPlan {
    pub files: Vec<PlannedFile>,
    pub total_bytes: u64,
    pub skipped: usize,
    pub empty_folders: Vec<String>,
}

/// 上传前规划：递归收集文件并统计大小，供前端确认框展示
#[tauri::command]
pub fn plan_upload(paths: Vec<String>, prefix: String) -> Result<UploadPlan, String> {
    let path_bufs: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
    let files = collect_upload_files(&path_bufs, &prefix);
    let mut planned = Vec::with_capacity(files.len());
    let mut total = 0u64;
    for (p, k) in files {
        let size = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
        total += size;
        planned.push(PlannedFile {
            path: p.to_string_lossy().into_owned(),
            key: k,
            size,
        });
    }
    if planned.is_empty() {
        return Err("没有找到可上传的文件".to_string());
    }
    Ok(UploadPlan {
        files: planned,
        total_bytes: total,
        skipped: 0,
        empty_folders: easys3_core::plan::collect_empty_folders(&path_bufs, &prefix),
    })
}

#[tauri::command]
pub fn paste_clipboard_image() -> Result<Option<String>, String> {
    let mut clipboard = arboard::Clipboard::new().map_err(|_| "无法读取剪贴板".to_string())?;
    let image = match clipboard.get_image() {
        Ok(image) => image,
        Err(_) => return Ok(None),
    };
    let dir = std::env::temp_dir().join("easys3-paste");
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建剪贴板临时目录失败：{e}"))?;
    let path = dir.join(format!(
        "paste-{}.png",
        chrono::Utc::now().format("%Y%m%d-%H%M%S")
    ));
    let mut file = std::fs::File::create(&path).map_err(|e| format!("保存剪贴板图片失败：{e}"))?;
    let image_buffer = image::RgbaImage::from_raw(
        image.width as u32,
        image.height as u32,
        image.bytes.into_owned(),
    )
    .ok_or("剪贴板图片格式无效")?;
    image_buffer
        .write_to(&mut file, image::ImageFormat::Png)
        .map_err(|e| format!("编码剪贴板图片失败：{e}"))?;
    Ok(Some(path.to_string_lossy().into_owned()))
}

#[tauri::command]
pub async fn plan_upload_changes(
    state: State<'_, App>,
    bucket: String,
    paths: Vec<String>,
    prefix: String,
) -> Result<UploadPlan, String> {
    let path_bufs: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
    let files = collect_upload_files(&path_bufs, &prefix);
    let (client, _) = state.current_control_client()?;
    let cancel = Arc::new(AtomicBool::new(false));
    let remote = easys3_core::list::collect_prefix_sizes(&client, &bucket, &prefix, &cancel)
        .await
        .map_err(|e| e.to_string())?;
    let mut planned = Vec::new();
    let mut total = 0;
    let mut skipped = 0;
    for (path, key) in files {
        let size = std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
        if remote.get(&key) == Some(&size) {
            skipped += 1;
        } else {
            total += size;
            planned.push(PlannedFile {
                path: path.to_string_lossy().into_owned(),
                key,
                size,
            });
        }
    }
    Ok(UploadPlan {
        files: planned,
        total_bytes: total,
        skipped,
        empty_folders: easys3_core::plan::collect_empty_folders(&path_bufs, &prefix),
    })
}

#[tauri::command]
pub fn start_upload(
    state: State<App>,
    app: tauri::AppHandle,
    bucket: String,
    prefix: String,
    paths: Vec<String>,
    storage: Option<String>,
    verify: bool,
) -> Result<String, String> {
    require_writable(&state)?;
    let connection = state.current_connection()?;
    let (client, _) = state.current_client()?;
    let path_bufs: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
    let files = collect_upload_files(&path_bufs, &prefix);
    if files.is_empty() {
        return Err("没有可上传的文件".to_string());
    }
    let bytes_total = tasks::upload_files_size(&files);
    let id = Uuid::new_v4().to_string();
    let cancel = Arc::new(AtomicBool::new(false));
    let shared = TaskShared::new(tasks::new_task_info(
        &id,
        TaskKind::Upload,
        &bucket,
        files.len(),
        bytes_total,
    ));
    let retry = Arc::new(Mutex::new(Some(RetryJob::Upload {
        client: client.clone(),
        bucket: bucket.clone(),
        files: files.clone(),
    })));
    {
        let mut inner = lock(&state.inner);
        inner.tasks.insert(
            id.clone(),
            TaskEntry {
                cancel: cancel.clone(),
                shared: shared.clone(),
                retry: retry.clone(),
            },
        );
    }
    let storage_class = storage
        .as_deref()
        .filter(|value| !value.is_empty() && *value != "STANDARD")
        .map(|value| value.parse::<aws_sdk_s3::types::StorageClass>())
        .transpose()
        .map_err(|_| "不支持的存储类别".to_string())?;
    let settings = state.transfer_settings();
    let storage_name = storage
        .clone()
        .filter(|value| !value.is_empty() && value != "STANDARD");
    {
        let mut resume = lock(&state.resume);
        resume.tasks.push(ResumeTask::Upload {
            id: id.clone(),
            connection_id: connection.id,
            bucket: bucket.clone(),
            files: files
                .iter()
                .map(|(path, key)| ResumeUploadFile {
                    path: path.to_string_lossy().into_owned(),
                    key: key.clone(),
                    upload_id: None,
                    part_size: 0,
                    completed_parts: Vec::new(),
                })
                .collect(),
            storage_class: storage_name,
            created_at: lock(&shared.info).created_at,
        });
    }
    state.save_resume()?;
    let resume = state.resume.clone();
    let resume_path = state.resume_path.clone();
    let task_id = id.clone();
    let checkpoints =
        Arc::new(
            move |key: &str, checkpoint: easys3_core::upload::MultipartCheckpoint| {
                {
                    let mut file = lock(&resume);
                    if let Some(ResumeTask::Upload { files, .. }) = file.tasks.iter_mut().find(
                        |task| matches!(task, ResumeTask::Upload { id, .. } if id == &task_id),
                    ) {
                        if let Some(item) = files.iter_mut().find(|item| item.key == key) {
                            item.upload_id = checkpoint.upload_id;
                            item.part_size = checkpoint.part_size;
                            item.completed_parts = checkpoint
                                .completed_parts
                                .into_iter()
                                .map(|(part_number, e_tag)| ResumePart { part_number, e_tag })
                                .collect();
                        }
                    }
                }
                save_resume_snapshot(&resume_path, &resume);
            },
        );
    let resume = state.resume.clone();
    let resume_path = state.resume_path.clone();
    let task_id = id.clone();
    let completed = Arc::new(move || remove_resume_task(&resume_path, &resume, &task_id));
    tasks::spawn_upload(
        app,
        client,
        bucket,
        files,
        cancel,
        shared,
        storage_class,
        settings,
        state.upload_throttle.clone(),
        verify,
        Some(checkpoints),
        None,
        Some(completed),
    );
    Ok(id)
}

#[tauri::command]
pub async fn start_upload_plan(
    state: State<'_, App>,
    app: tauri::AppHandle,
    bucket: String,
    plan: UploadPlan,
    storage: Option<String>,
    verify: bool,
) -> Result<String, String> {
    require_writable(&state)?;
    let files: Vec<_> = plan
        .files
        .iter()
        .map(|file| (PathBuf::from(&file.path), file.key.clone()))
        .collect();
    if files.is_empty() && plan.empty_folders.is_empty() {
        return Err("没有需要上传的文件".to_string());
    }
    let (client, _) = state.current_client()?;
    if !plan.empty_folders.is_empty() {
        easys3_core::object::create_empty_folders(&client, &bucket, &plan.empty_folders)
            .await
            .map_err(|e| e.to_string())?;
    }
    if files.is_empty() {
        return Ok(String::new());
    }
    let id = Uuid::new_v4().to_string();
    let cancel = Arc::new(AtomicBool::new(false));
    let mut info = tasks::new_task_info(
        &id,
        TaskKind::Upload,
        &bucket,
        files.len() + plan.empty_folders.len(),
        tasks::upload_files_size(&files),
    );
    info.files_done = plan.empty_folders.len();
    info.files_skipped = plan.skipped;
    let shared = TaskShared::new(info);
    let retry = Arc::new(Mutex::new(Some(RetryJob::Upload {
        client: client.clone(),
        bucket: bucket.clone(),
        files: files.clone(),
    })));
    lock(&state.inner).tasks.insert(
        id.clone(),
        TaskEntry {
            cancel: cancel.clone(),
            shared: shared.clone(),
            retry,
        },
    );
    let class = storage
        .as_deref()
        .filter(|value| !value.is_empty() && *value != "STANDARD")
        .map(str::parse)
        .transpose()
        .map_err(|_| "不支持的存储类别".to_string())?;
    tasks::spawn_upload(
        app,
        client,
        bucket,
        files,
        cancel,
        shared,
        class,
        state.transfer_settings(),
        state.upload_throttle.clone(),
        verify,
        None,
        None,
        None,
    );
    Ok(id)
}

#[tauri::command]
pub fn start_download(
    state: State<App>,
    app: tauri::AppHandle,
    bucket: String,
    items: Vec<FsItem>,
    dest: String,
    conflict: String,
) -> Result<String, String> {
    let connection = state.current_connection()?;
    // 传输客户端下载对象体；控制面客户端展开列出（带超时，规格）
    let (client, _) = state.current_client()?;
    let (control, _) = state.current_control_client()?;
    let id = Uuid::new_v4().to_string();
    let cancel = Arc::new(AtomicBool::new(false));
    let shared = TaskShared::new(tasks::new_task_info(
        &id,
        TaskKind::Download,
        &bucket,
        items.len(),
        0,
    ));
    let retry = Arc::new(Mutex::new(None));
    {
        let mut inner = lock(&state.inner);
        inner.tasks.insert(
            id.clone(),
            TaskEntry {
                cancel: cancel.clone(),
                shared: shared.clone(),
                retry: retry.clone(),
            },
        );
    }
    {
        let mut resume = lock(&state.resume);
        resume.tasks.push(ResumeTask::Download {
            id: id.clone(),
            connection_id: connection.id,
            bucket: bucket.clone(),
            items: items
                .iter()
                .map(|item| easys3_core::resume::ResumeItem {
                    key: item.key.clone(),
                    is_dir: item.is_dir,
                })
                .collect(),
            dest: dest.clone(),
            conflict: conflict.clone(),
            created_at: lock(&shared.info).created_at,
        });
    }
    state.save_resume()?;
    let resume = state.resume.clone();
    let resume_path = state.resume_path.clone();
    let task_id = id.clone();
    let completed = Arc::new(move || remove_resume_task(&resume_path, &resume, &task_id));
    tasks::spawn_download_expand(
        app,
        client,
        control,
        bucket,
        items,
        PathBuf::from(dest),
        conflict,
        None,
        cancel,
        shared,
        retry,
        state.transfer_settings(),
        state.download_throttle.clone(),
        true,
        Some(completed),
    );
    Ok(id)
}

#[tauri::command]
pub fn start_delete(
    state: State<App>,
    app: tauri::AppHandle,
    bucket: String,
    items: Vec<FsItem>,
) -> Result<String, String> {
    require_writable(&state)?;
    // 删除任务用控制面客户端：展开列出与 DeleteObjects 均为小请求（带超时，规格）
    let (client, _) = state.current_control_client()?;
    let id = Uuid::new_v4().to_string();
    let cancel = Arc::new(AtomicBool::new(false));
    let shared = TaskShared::new(tasks::new_task_info(
        &id,
        TaskKind::Delete,
        &bucket,
        items.len(),
        0,
    ));
    let retry = Arc::new(Mutex::new(None));
    {
        let mut inner = lock(&state.inner);
        inner.tasks.insert(
            id.clone(),
            TaskEntry {
                cancel: cancel.clone(),
                shared: shared.clone(),
                retry: retry.clone(),
            },
        );
    }
    tasks::spawn_delete_expand(app, client, bucket, items, None, cancel, shared, retry);
    Ok(id)
}

#[tauri::command]
pub fn cancel_task(state: State<App>, task_id: String) -> Result<(), String> {
    let inner = lock(&state.inner);
    let entry = inner.tasks.get(&task_id).ok_or("任务不存在")?;
    entry
        .cancel
        .store(true, std::sync::atomic::Ordering::Relaxed);
    // 主动取消不应在下次启动继续；上传执行器会观察取消标志并 Abort multipart。
    drop(inner);
    remove_resume_task(&state.resume_path, &state.resume, &task_id);
    Ok(())
}

/// 手动重试失败文件（规格：以原参数重新开始该文件，不自动重试）
#[tauri::command]
pub fn retry_task(state: State<App>, app: tauri::AppHandle, task_id: String) -> Result<(), String> {
    // 先取出任务数据再获取客户端，避免双锁死锁
    let (shared, retry_job) = {
        let inner = lock(&state.inner);
        let entry = inner.tasks.get(&task_id).ok_or("任务不存在")?;
        let retry_job = entry
            .retry
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        (entry.shared.clone(), retry_job)
    };
    {
        let gi = lock(&shared.info);
        if matches!(gi.status, TaskStatus::Running | TaskStatus::Queued) {
            return Err("任务仍在进行中".to_string());
        }
        if gi.failures.is_empty() {
            return Err("没有可重试的失败项".to_string());
        }
    }
    let failed: HashSet<String> = lock(&shared.info)
        .failures
        .iter()
        .map(|f| f.key.clone())
        .collect();
    let cancel = Arc::new(AtomicBool::new(false));

    match retry_job {
        Some(RetryJob::Upload {
            client,
            bucket,
            files,
        }) => {
            let subset: Vec<_> = files
                .into_iter()
                .filter(|(_, k)| failed.contains(k))
                .collect();
            if subset.is_empty() {
                return Err("没有可重试的失败项".to_string());
            }
            let bytes_total = tasks::upload_files_size(&subset);
            {
                let mut gi = lock(&shared.info);
                gi.status = TaskStatus::Running;
                gi.files_total = subset.len();
                gi.files_done = 0;
                gi.files_failed = 0;
                gi.files_skipped = 0;
                gi.bytes_total = bytes_total;
                gi.bytes_done = 0;
                gi.failures.clear();
                gi.current_file = None;
                gi.retry_count = 0;
            }
            {
                let mut inner = lock(&state.inner);
                if let Some(entry) = inner.tasks.get_mut(&task_id) {
                    entry.cancel = cancel.clone();
                    *entry.retry.lock().unwrap_or_else(|e| e.into_inner()) =
                        Some(RetryJob::Upload {
                            client: client.clone(),
                            bucket: bucket.clone(),
                            files: subset.clone(),
                        });
                }
            }
            let settings = state.transfer_settings();
            tasks::spawn_upload(
                app,
                client,
                bucket,
                subset,
                cancel,
                shared,
                None,
                settings,
                state.upload_throttle.clone(),
                false,
                None,
                None,
                None,
            );
        }
        Some(RetryJob::SyncDownload {
            client,
            bucket,
            jobs,
        }) => {
            let subset: Vec<_> = jobs
                .into_iter()
                .filter(|job| failed.contains(&job.key))
                .collect();
            if subset.is_empty() {
                return Err("没有可重试的失败项".to_string());
            }
            {
                let mut gi = lock(&shared.info);
                gi.status = TaskStatus::Running;
                gi.files_total = subset.len();
                gi.files_done = 0;
                gi.files_failed = 0;
                gi.files_skipped = 0;
                gi.bytes_total = 0;
                gi.bytes_done = 0;
                gi.failures.clear();
                gi.current_file = None;
                gi.retry_count = 0;
            }
            let retry_slot = {
                let mut inner = lock(&state.inner);
                match inner.tasks.get_mut(&task_id) {
                    Some(entry) => {
                        entry.cancel = cancel.clone();
                        entry.retry.clone()
                    }
                    None => Arc::new(Mutex::new(None)),
                }
            };
            *lock(&retry_slot) = Some(RetryJob::SyncDownload {
                client: client.clone(),
                bucket: bucket.clone(),
                jobs: subset.clone(),
            });
            tasks::spawn_sync_download(
                app,
                client,
                bucket,
                subset,
                cancel,
                shared,
                state.transfer_settings(),
                state.download_throttle.clone(),
            );
        }
        Some(RetryJob::SyncLocalDelete { bucket, paths }) => {
            let subset: Vec<_> = paths
                .into_iter()
                .filter(|path| failed.contains(&path.to_string_lossy().into_owned()))
                .collect();
            if subset.is_empty() {
                return Err("没有可重试的失败项".to_string());
            }
            {
                let mut gi = lock(&shared.info);
                gi.status = TaskStatus::Running;
                gi.files_total = subset.len();
                gi.files_done = 0;
                gi.files_failed = 0;
                gi.files_skipped = 0;
                gi.bytes_total = 0;
                gi.bytes_done = 0;
                gi.failures.clear();
                gi.current_file = None;
                gi.retry_count = 0;
            }
            let retry_slot = {
                let mut inner = lock(&state.inner);
                match inner.tasks.get_mut(&task_id) {
                    Some(entry) => {
                        entry.cancel = cancel.clone();
                        entry.retry.clone()
                    }
                    None => Arc::new(Mutex::new(None)),
                }
            };
            *lock(&retry_slot) = Some(RetryJob::SyncLocalDelete {
                bucket,
                paths: subset.clone(),
            });
            tasks::spawn_sync_local_delete(app, subset, cancel, shared);
        }
        Some(RetryJob::Download {
            client,
            control,
            bucket,
            items,
            dest_dir,
            conflict,
        }) => {
            // 失败项命中的列表项：文件直接重试；文件夹（含展开失败的）重新展开后
            // 仅重试其中失败对象（spawn 内过滤，已成功的不重复下载）
            let subset: Vec<FsItem> = items
                .into_iter()
                .filter(|i| item_matches_failure(&i.key, i.is_dir, &failed))
                .collect();
            if subset.is_empty() {
                return Err("没有可重试的失败项".to_string());
            }
            {
                let mut gi = lock(&shared.info);
                gi.status = TaskStatus::Running;
                gi.files_total = subset.len();
                gi.files_done = 0;
                gi.files_failed = 0;
                gi.files_skipped = 0;
                gi.bytes_total = 0;
                gi.bytes_done = 0;
                gi.failures.clear();
                gi.current_file = None;
                gi.retry_count = 0;
            }
            // 更新取消标志并取回重试槽；任务条目被并发清除时用一次性槽位，重试仍执行
            let retry_slot = {
                let mut inner = lock(&state.inner);
                match inner.tasks.get_mut(&task_id) {
                    Some(entry) => {
                        entry.cancel = cancel.clone();
                        entry.retry.clone()
                    }
                    None => Arc::new(Mutex::new(None)),
                }
            };
            tasks::spawn_download_expand(
                app,
                client,
                control,
                bucket,
                subset,
                dest_dir,
                conflict,
                Some(failed),
                cancel,
                shared,
                retry_slot,
                state.transfer_settings(),
                state.download_throttle.clone(),
                false,
                None,
            );
        }
        Some(RetryJob::Delete {
            client,
            bucket,
            items,
        }) => {
            // 失败项命中的列表项：文件直接重试；文件夹（含展开失败的）重新展开后
            // 仅重试其中失败 key（spawn 内过滤，已成功的不重复删除）
            let subset: Vec<FsItem> = items
                .into_iter()
                .filter(|i| item_matches_failure(&i.key, i.is_dir, &failed))
                .collect();
            if subset.is_empty() {
                return Err("没有可重试的失败项".to_string());
            }
            {
                let mut gi = lock(&shared.info);
                gi.status = TaskStatus::Running;
                gi.files_total = subset.len();
                gi.files_done = 0;
                gi.files_failed = 0;
                gi.files_skipped = 0;
                gi.bytes_total = 0;
                gi.bytes_done = 0;
                gi.failures.clear();
                gi.current_file = None;
                gi.retry_count = 0;
            }
            // 更新取消标志并取回重试槽；任务条目被并发清除时用一次性槽位，重试仍执行
            let retry_slot = {
                let mut inner = lock(&state.inner);
                match inner.tasks.get_mut(&task_id) {
                    Some(entry) => {
                        entry.cancel = cancel.clone();
                        entry.retry.clone()
                    }
                    None => Arc::new(Mutex::new(None)),
                }
            };
            tasks::spawn_delete_expand(
                app,
                client,
                bucket,
                subset,
                Some(failed),
                cancel,
                shared,
                retry_slot,
            );
        }
        Some(RetryJob::Copy {
            client,
            bucket,
            target_bucket,
            items,
            target_prefix,
            new_name,
            conflict,
            remove,
        }) => {
            // 失败项命中的列表项：文件直接重试；文件夹（含展开失败的）重新展开后
            // 仅重试其中失败 key（spawn 内过滤，已成功的不重复复制）
            let subset: Vec<FsItem> = items
                .into_iter()
                .filter(|i| item_matches_failure(&i.key, i.is_dir, &failed))
                .collect();
            if subset.is_empty() {
                return Err("没有可重试的失败项".to_string());
            }
            {
                let mut gi = lock(&shared.info);
                gi.status = TaskStatus::Running;
                gi.files_total = subset.len();
                gi.files_done = 0;
                gi.files_failed = 0;
                gi.files_skipped = 0;
                gi.bytes_total = 0;
                gi.bytes_done = 0;
                gi.failures.clear();
                gi.current_file = None;
                gi.retry_count = 0;
            }
            // 更新取消标志并取回重试槽；任务条目被并发清除时用一次性槽位，重试仍执行
            let retry_slot = {
                let mut inner = lock(&state.inner);
                match inner.tasks.get_mut(&task_id) {
                    Some(entry) => {
                        entry.cancel = cancel.clone();
                        entry.retry.clone()
                    }
                    None => Arc::new(Mutex::new(None)),
                }
            };
            tasks::spawn_copy(
                app,
                client,
                bucket,
                target_bucket,
                subset,
                target_prefix,
                conflict,
                new_name,
                remove,
                Some(failed),
                cancel,
                shared,
                retry_slot,
                state.transfer_settings(),
            );
        }
        None => return Err("该任务不支持重试".to_string()),
    }
    Ok(())
}

#[tauri::command]
pub fn get_tasks(state: State<App>) -> Vec<TaskInfo> {
    let inner = lock(&state.inner);
    let mut infos: Vec<TaskInfo> = inner
        .tasks
        .values()
        .map(|e| {
            e.shared
                .info
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone()
        })
        .collect();
    infos.sort_by_key(|i| i.created_at);
    infos
}

#[tauri::command]
pub fn clear_finished_tasks(state: State<App>) -> Result<(), String> {
    let mut inner = lock(&state.inner);
    // 锁毒化按“仍在运行”处理（保留条目），与项目防毒化约定一致，避免误清运行中任务；
    // 排队中（Queued）的任务尚未开始，同样保留
    inner.tasks.retain(|_, e| {
        matches!(
            lock(&e.shared.info).status,
            TaskStatus::Running | TaskStatus::Queued
        )
    });
    Ok(())
}
