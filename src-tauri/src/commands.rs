//! Tauri 命令层：前端 invoke 入口。
//! 错误一律返回中文可读字符串（easys3-core 已保证不含 secret_key）。

use crate::state::{lock, App};
use crate::tasks::{self, FsItem, RetryJob, TaskEntry, TaskInfo, TaskKind, TaskShared, TaskStatus};
use easys3_core::config::{connection_fields_changed, validate_connection, ConnectionConfig};
use easys3_core::list::ListResult;
use easys3_core::plan::{collect_upload_files, item_matches_failure};
use easys3_core::preview::PreviewData;
use easys3_core::settings::{save_settings, TransferSettings};
use serde::Serialize;
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
pub fn is_download_directory(dest: String) -> bool {
    std::path::Path::new(&dest).is_dir()
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
pub async fn create_folder(
    state: State<'_, App>,
    bucket: String,
    prefix: String,
    name: String,
) -> Result<(), String> {
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
    conflict: String,
    new_name: Option<String>,
    remove: bool,
) -> Result<String, String> {
    if let Some(name) = new_name.as_deref() {
        if name.is_empty() || name.contains('/') {
            return Err("新名称不能为空且不能包含 /".to_string());
        }
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

#[derive(Serialize)]
pub struct PlannedFile {
    pub path: String,
    pub key: String,
    pub size: u64,
}

#[derive(Serialize)]
pub struct UploadPlan {
    pub files: Vec<PlannedFile>,
    pub total_bytes: u64,
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
) -> Result<String, String> {
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
            );
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
