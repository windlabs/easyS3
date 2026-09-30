//! 传输任务执行器：上传 / 下载 / 删除的后台运行、进度事件、取消、失败重试。
//! 规格：`.agents/requirements.md`「传输任务中心」、`.agents/s3-operations.md` 通用约束。
//!
//! 重试语义（规格）：不自动重试；任务中心手动重试，以原参数重新开始失败文件。

use easys3_core::delete::delete_keys;
use easys3_core::download::download_object;
use easys3_core::error::CoreError;
use easys3_core::list::collect_prefix_objects;
use easys3_core::plan::unique_path;
use easys3_core::upload::{upload_file, ProgressFn};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::Emitter;

/// 文件间并发（规格：默认 2，常量可调）
pub const UPLOAD_CONCURRENCY: usize = 2;
/// 进度事件节流间隔
const EMIT_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskKind {
    Upload,
    Download,
    Delete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Running,
    Done,
    Failed,
    Canceled,
}

#[derive(Debug, Clone, Serialize)]
pub struct Failure {
    pub key: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskInfo {
    pub id: String,
    pub kind: TaskKind,
    pub bucket: String,
    pub status: TaskStatus,
    pub files_total: usize,
    pub files_done: usize,
    pub files_failed: usize,
    pub files_skipped: usize,
    pub bytes_total: u64,
    pub bytes_done: u64,
    pub current_file: Option<String>,
    pub failures: Vec<Failure>,
    pub created_at: u64,
}

/// 前端传入的条目（文件 key 或目录前缀）
#[derive(Debug, Clone, Deserialize)]
pub struct FsItem {
    pub key: String,
    pub is_dir: bool,
}

pub struct TaskShared {
    pub info: Mutex<TaskInfo>,
    last_emit: Mutex<Option<Instant>>,
}

impl TaskShared {
    pub fn new(info: TaskInfo) -> Arc<Self> {
        Arc::new(TaskShared {
            info: Mutex::new(info),
            last_emit: Mutex::new(None),
        })
    }
}

#[derive(Debug, Clone)]
pub struct DownloadJob {
    pub key: String,
    pub dest: PathBuf,
}

/// 重试所需的原始参数（客户端快照：任务与连接解耦，切换/删除连接不中断）
#[derive(Clone)]
pub enum RetryJob {
    Upload {
        client: aws_sdk_s3::Client,
        bucket: String,
        files: Vec<(PathBuf, String)>,
    },
    Download {
        client: aws_sdk_s3::Client,
        bucket: String,
        jobs: Vec<DownloadJob>,
        conflict: String,
    },
    Delete {
        client: aws_sdk_s3::Client,
        bucket: String,
        keys: Vec<String>,
    },
}

pub struct TaskEntry {
    pub cancel: Arc<AtomicBool>,
    pub shared: Arc<TaskShared>,
    pub retry: Arc<Mutex<Option<RetryJob>>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn new_task_info(
    id: &str,
    kind: TaskKind,
    bucket: &str,
    files_total: usize,
    bytes_total: u64,
) -> TaskInfo {
    TaskInfo {
        id: id.to_string(),
        kind,
        bucket: bucket.to_string(),
        status: TaskStatus::Running,
        files_total,
        files_done: 0,
        files_failed: 0,
        files_skipped: 0,
        bytes_total,
        bytes_done: 0,
        current_file: None,
        failures: Vec::new(),
        created_at: now_ms(),
    }
}

/// 上传文件的总大小（用于任务进度分母）
pub fn upload_files_size(files: &[(PathBuf, String)]) -> u64 {
    files
        .iter()
        .map(|(p, _)| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0))
        .sum()
}

fn emit_task(app: &tauri::AppHandle, shared: &TaskShared, force: bool) {
    {
        let mut le = lock(&shared.last_emit);
        if !force {
            if let Some(t) = *le {
                if t.elapsed() < EMIT_INTERVAL {
                    return;
                }
            }
        }
        *le = Some(Instant::now());
    }
    let info = lock(&shared.info).clone();
    let _ = app.emit("task-update", info);
}

fn finish_task(app: &tauri::AppHandle, cancel: &Arc<AtomicBool>, shared: &Arc<TaskShared>) {
    {
        let mut gi = lock(&shared.info);
        gi.current_file = None;
        gi.status = if cancel.load(Ordering::Relaxed) {
            TaskStatus::Canceled
        } else if !gi.failures.is_empty() {
            TaskStatus::Failed
        } else {
            TaskStatus::Done
        };
    }
    emit_task(app, shared, true);
}

/// 进度回调：按增量累加到任务级 bytes_done（多文件并发下正确求和）
fn make_delta_progress(app: &tauri::AppHandle, shared: &Arc<TaskShared>) -> ProgressFn {
    let s = shared.clone();
    let ah = app.clone();
    let last = Arc::new(Mutex::new(0u64));
    Arc::new(move |done: u64, _total: u64| {
        let delta = {
            let mut l = lock(&last);
            let d = done.saturating_sub(*l);
            *l = done;
            d
        };
        if delta > 0 {
            lock(&s.info).bytes_done += delta;
        }
        emit_task(&ah, &s, false);
    })
}

/// 下载进度回调：首个回调时把该文件大小计入任务总大小（避免逐文件 Head 预取）
fn make_growing_progress(app: &tauri::AppHandle, shared: &Arc<TaskShared>) -> ProgressFn {
    let s = shared.clone();
    let ah = app.clone();
    let last = Arc::new(Mutex::new(0u64));
    let total_added = Arc::new(Mutex::new(false));
    Arc::new(move |done: u64, total: u64| {
        let delta = {
            let mut l = lock(&last);
            let d = done.saturating_sub(*l);
            *l = done;
            d
        };
        {
            let mut gi = lock(&s.info);
            let mut ta = lock(&total_added);
            if !*ta {
                *ta = true;
                gi.bytes_total += total;
            }
            gi.bytes_done += delta;
        }
        emit_task(&ah, &s, false);
    })
}

fn record_failure(shared: &Arc<TaskShared>, key: &str, error: String) {
    let mut gi = lock(&shared.info);
    gi.files_failed += 1;
    gi.failures.push(Failure {
        key: key.to_string(),
        error,
    });
}

// ---------- 上传 ----------

pub fn spawn_upload(
    app: tauri::AppHandle,
    client: aws_sdk_s3::Client,
    bucket: String,
    files: Vec<(PathBuf, String)>,
    cancel: Arc<AtomicBool>,
    shared: Arc<TaskShared>,
) {
    tauri::async_runtime::spawn(async move {
        let queue = Arc::new(Mutex::new(VecDeque::from(files)));
        let mut workers = Vec::new();
        for _ in 0..UPLOAD_CONCURRENCY {
            let q = queue.clone();
            let c = cancel.clone();
            let s = shared.clone();
            let cl = client.clone();
            let b = bucket.clone();
            let ah = app.clone();
            workers.push(tauri::async_runtime::spawn(async move {
                loop {
                    if c.load(Ordering::Relaxed) {
                        break;
                    }
                    let item = { lock(&q).pop_front() };
                    let Some((path, key)) = item else { break };
                    lock(&s.info).current_file = Some(key.clone());
                    let progress = make_delta_progress(&ah, &s);
                    match upload_file(&cl, &b, &key, &path, progress, c.clone()).await {
                        Ok(()) => {
                            lock(&s.info).files_done += 1;
                        }
                        Err(CoreError::Cancelled) => break,
                        Err(e) => record_failure(&s, &key, e.to_string()),
                    }
                    emit_task(&ah, &s, false);
                }
            }));
        }
        for w in workers {
            let _ = w.await;
        }
        finish_task(&app, &cancel, &shared);
    });
}

// ---------- 下载 ----------

pub fn spawn_download_expand(
    app: tauri::AppHandle,
    client: aws_sdk_s3::Client,
    bucket: String,
    items: Vec<FsItem>,
    dest_dir: PathBuf,
    conflict: String,
    cancel: Arc<AtomicBool>,
    shared: Arc<TaskShared>,
    retry_slot: Arc<Mutex<Option<RetryJob>>>,
) {
    tauri::async_runtime::spawn(async move {
        // 展开：文件夹（前缀）递归列出全部对象，保留目录结构（规格）
        let mut jobs: Vec<DownloadJob> = Vec::new();
        for item in items {
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            if item.is_dir {
                let prefix = item.key.clone();
                match collect_prefix_objects(&client, &bucket, &prefix, &cancel).await {
                    Ok(keys) => jobs.extend(keys.into_iter().map(|k| {
                        let rel = k
                            .strip_prefix(prefix.as_str())
                            .unwrap_or(k.as_str())
                            .to_owned();
                        DownloadJob {
                            key: k,
                            dest: dest_dir.join(rel),
                        }
                    })),
                    Err(CoreError::Cancelled) => break,
                    Err(e) => record_failure(&shared, &prefix, format!("列出对象失败：{e}")),
                }
            } else {
                let name = item.key.rsplit('/').next().unwrap_or(&item.key);
                jobs.push(DownloadJob {
                    key: item.key.clone(),
                    dest: dest_dir.join(name),
                });
            }
        }
        {
            let mut gi = lock(&shared.info);
            gi.files_total = jobs.len() + gi.files_failed;
        }
        *lock(&retry_slot) = Some(RetryJob::Download {
            client: client.clone(),
            bucket: bucket.clone(),
            jobs: jobs.clone(),
            conflict: conflict.clone(),
        });
        if !cancel.load(Ordering::Relaxed) && !jobs.is_empty() {
            run_download_jobs(&app, &client, &bucket, jobs, &conflict, &cancel, &shared).await;
        } else {
            finish_task(&app, &cancel, &shared);
        }
    });
}

pub fn spawn_download_jobs(
    app: tauri::AppHandle,
    client: aws_sdk_s3::Client,
    bucket: String,
    jobs: Vec<DownloadJob>,
    conflict: String,
    cancel: Arc<AtomicBool>,
    shared: Arc<TaskShared>,
) {
    tauri::async_runtime::spawn(async move {
        run_download_jobs(&app, &client, &bucket, jobs, &conflict, &cancel, &shared).await;
    });
}

async fn run_download_jobs(
    app: &tauri::AppHandle,
    client: &aws_sdk_s3::Client,
    bucket: &str,
    jobs: Vec<DownloadJob>,
    conflict: &str,
    cancel: &Arc<AtomicBool>,
    shared: &Arc<TaskShared>,
) {
    for job in jobs {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        // 同名冲突策略（规格：覆盖 / 自动重命名 name (1).ext / 跳过）
        let dest = match conflict {
            "skip" => {
                if job.dest.exists() {
                    lock(&shared.info).files_skipped += 1;
                    continue;
                }
                job.dest.clone()
            }
            "rename" => unique_path(&job.dest),
            _ => job.dest.clone(), // overwrite
        };
        lock(&shared.info).current_file = Some(job.key.clone());
        let progress = make_growing_progress(app, shared);
        match download_object(client, bucket, &job.key, &dest, progress, cancel.clone()).await {
            Ok(()) => {
                lock(&shared.info).files_done += 1;
            }
            Err(CoreError::Cancelled) => break,
            Err(e) => record_failure(shared, &job.key, e.to_string()),
        }
        emit_task(app, shared, false);
    }
    finish_task(app, cancel, shared);
}

// ---------- 删除 ----------

pub fn spawn_delete_expand(
    app: tauri::AppHandle,
    client: aws_sdk_s3::Client,
    bucket: String,
    items: Vec<FsItem>,
    cancel: Arc<AtomicBool>,
    shared: Arc<TaskShared>,
    retry_slot: Arc<Mutex<Option<RetryJob>>>,
) {
    tauri::async_runtime::spawn(async move {
        let mut keys: Vec<String> = Vec::new();
        for item in items {
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            if item.is_dir {
                let prefix = item.key.clone();
                match collect_prefix_objects(&client, &bucket, &prefix, &cancel).await {
                    Ok(ks) => keys.extend(ks),
                    Err(CoreError::Cancelled) => break,
                    Err(e) => record_failure(&shared, &prefix, format!("列出对象失败：{e}")),
                }
            } else {
                keys.push(item.key.clone());
            }
        }
        {
            let mut gi = lock(&shared.info);
            gi.files_total = keys.len() + gi.files_failed;
        }
        *lock(&retry_slot) = Some(RetryJob::Delete {
            client: client.clone(),
            bucket: bucket.clone(),
            keys: keys.clone(),
        });
        if !cancel.load(Ordering::Relaxed) && !keys.is_empty() {
            run_delete_keys(&app, &client, &bucket, keys, &cancel, &shared).await;
        } else {
            finish_task(&app, &cancel, &shared);
        }
    });
}

pub fn spawn_delete_keys(
    app: tauri::AppHandle,
    client: aws_sdk_s3::Client,
    bucket: String,
    keys: Vec<String>,
    cancel: Arc<AtomicBool>,
    shared: Arc<TaskShared>,
) {
    tauri::async_runtime::spawn(async move {
        run_delete_keys(&app, &client, &bucket, keys, &cancel, &shared).await;
    });
}

async fn run_delete_keys(
    app: &tauri::AppHandle,
    client: &aws_sdk_s3::Client,
    bucket: &str,
    keys: Vec<String>,
    cancel: &Arc<AtomicBool>,
    shared: &Arc<TaskShared>,
) {
    match delete_keys(client, bucket, &keys, cancel.clone()).await {
        Ok(report) => {
            let mut gi = lock(&shared.info);
            gi.files_done = report.deleted;
            gi.failures = report
                .failures
                .iter()
                .map(|f| Failure {
                    key: f.key.clone(),
                    error: f.message.clone(),
                })
                .collect();
            gi.files_failed = gi.failures.len();
        }
        Err(CoreError::Cancelled) => {}
        Err(e) => record_failure(shared, "-", e.to_string()),
    }
    finish_task(app, cancel, shared);
}
