//! Tauri 命令层：前端 invoke 入口。
//! 错误一律返回中文可读字符串（easys3-core 已保证不含 secret_key）。

use crate::state::{lock, App};
use crate::tasks::{self, FsItem, RetryJob, TaskEntry, TaskInfo, TaskKind, TaskShared, TaskStatus};
use easys3_core::config::{connection_fields_changed, validate_connection, ConnectionConfig};
use easys3_core::list::ListResult;
use easys3_core::plan::collect_upload_files;
use easys3_core::preview::PreviewData;
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
    StateDto {
        connections: inner.file.connections.clone(),
        current_connection_id: inner.file.current_connection_id.clone(),
    }
}

#[tauri::command]
pub fn save_connection(state: State<App>, connection: ConnectionConfig) -> Result<(), String> {
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
                // 连接相关字段变更后，已持久化的测试结果作废清空（规格）
                if connection_fields_changed(existing, &updated) {
                    updated.last_test = None;
                }
                *existing = updated;
            }
            None => inner.file.connections.push(connection.clone()),
        }
        // 配置变更后重建该连接的 S3 客户端
        inner.clients.remove(&connection.id);
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
        // 删除当前连接：自动选中剩余第一个，无连接则为空（规格）
        if inner.file.current_connection_id.as_deref() == Some(id.as_str()) {
            inner.file.current_connection_id = inner.file.connections.first().map(|p| p.id.clone());
        }
    }
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
pub async fn test_connection(connection: ConnectionConfig) -> Result<(), String> {
    easys3_core::s3::test_connection(&connection)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_buckets(state: State<'_, App>) -> Result<Vec<String>, String> {
    let (client, _) = state.current_client()?;
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
    let (client, _) = state.current_client()?;
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
    let (client, _) = state.current_client()?;
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
    let (client, _) = state.current_client()?;
    easys3_core::preview::preview_object(&client, &bucket, &key)
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
    tasks::spawn_upload(app, client, bucket, files, cancel, shared);
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
    let (client, _) = state.current_client()?;
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
        bucket,
        items,
        PathBuf::from(dest),
        conflict,
        cancel,
        shared,
        retry,
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
    let (client, _) = state.current_client()?;
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
    tasks::spawn_delete_expand(app, client, bucket, items, cancel, shared, retry);
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
        if gi.status == TaskStatus::Running {
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
            tasks::spawn_upload(app, client, bucket, subset, cancel, shared);
        }
        Some(RetryJob::Download {
            client,
            bucket,
            jobs,
            conflict,
        }) => {
            let subset: Vec<_> = jobs
                .into_iter()
                .filter(|j| failed.contains(&j.key))
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
            }
            {
                let mut inner = lock(&state.inner);
                if let Some(entry) = inner.tasks.get_mut(&task_id) {
                    entry.cancel = cancel.clone();
                    *entry.retry.lock().unwrap_or_else(|e| e.into_inner()) =
                        Some(RetryJob::Download {
                            client: client.clone(),
                            bucket: bucket.clone(),
                            jobs: subset.clone(),
                            conflict: conflict.clone(),
                        });
                }
            }
            tasks::spawn_download_jobs(app, client, bucket, subset, conflict, cancel, shared);
        }
        Some(RetryJob::Delete {
            client,
            bucket,
            keys,
        }) => {
            let subset: Vec<_> = keys.into_iter().filter(|k| failed.contains(k)).collect();
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
            }
            {
                let mut inner = lock(&state.inner);
                if let Some(entry) = inner.tasks.get_mut(&task_id) {
                    entry.cancel = cancel.clone();
                    *entry.retry.lock().unwrap_or_else(|e| e.into_inner()) =
                        Some(RetryJob::Delete {
                            client: client.clone(),
                            bucket: bucket.clone(),
                            keys: subset.clone(),
                        });
                }
            }
            tasks::spawn_delete_keys(app, client, bucket, subset, cancel, shared);
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
    inner.tasks.retain(|_, e| {
        e.shared
            .info
            .lock()
            .map(|i| i.status == TaskStatus::Running)
            .unwrap_or(false)
    });
    Ok(())
}
