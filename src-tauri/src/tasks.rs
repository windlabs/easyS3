//! 传输任务执行器：上传 / 下载 / 删除的后台运行、进度事件、取消、失败重试。
//! 规格：`.agents/requirements.md`「传输任务中心」、`.agents/s3-operations.md` 通用约束。
//!
//! 重试语义（规格）：瞬时错误自动退避重试（次数可配，默认 3）；
//! 任务中心手动重试以原参数重新开始失败文件，不重复执行已成功对象。

use easys3_core::delete::delete_keys;
use easys3_core::download::download_object;
use easys3_core::error::CoreError;
use easys3_core::list::collect_prefix_objects;
use easys3_core::plan::{key_matches_failure, unique_path};
use easys3_core::settings::TransferSettings;
use easys3_core::throttle::Throttle;
use easys3_core::upload::{upload_file, ProgressFn, UploadOptions};
use serde::{Deserialize, Serialize};
use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::Emitter;

/// 进度事件节流间隔
const EMIT_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskKind {
    Upload,
    Download,
    Delete,
    /// 桶内复制 / 移动 / 重命名（服务端 copy_object，规格 §6）
    Copy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    /// 达到最大并发任务数，排队等待槽位
    Queued,
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
    /// 瞬时错误自动重试累计次数（任务中心展示）
    pub retry_count: usize,
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

/// 桶内复制 / 移动的单个任务项
#[derive(Debug, Clone)]
pub struct CopyJob {
    pub source: String,
    pub target: String,
    /// true = 移动（复制成功后删除源对象）
    pub is_move: bool,
}

/// 重试所需的原始参数（客户端快照：任务与连接解耦，切换/删除连接不中断）。
/// Download / Delete / Copy 保存**原始列表项**而非展开结果——重试时按原参数重新展开，
/// 使展开失败（列出对象失败）的文件夹也能重试（规格）。
#[derive(Clone)]
pub enum RetryJob {
    Upload {
        client: aws_sdk_s3::Client,
        bucket: String,
        files: Vec<(PathBuf, String)>,
    },
    Download {
        /// 传输客户端：对象体下载（不设超时，大文件合法地慢）
        client: aws_sdk_s3::Client,
        /// 控制面客户端：展开列出（30s 尝试超时）
        control: aws_sdk_s3::Client,
        bucket: String,
        items: Vec<FsItem>,
        dest_dir: PathBuf,
        conflict: String,
    },
    Delete {
        /// 控制面客户端：展开列出与 DeleteObjects 均为小请求
        client: aws_sdk_s3::Client,
        bucket: String,
        items: Vec<FsItem>,
    },
    Copy {
        client: aws_sdk_s3::Client,
        bucket: String,
        items: Vec<FsItem>,
        target_prefix: String,
        /// 单对象重命名（仅 items 为单个文件时有效）
        new_name: Option<String>,
        conflict: String,
        /// true = 移动（复制成功后删除源对象）
        remove: bool,
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
        retry_count: 0,
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
    // 释放并发槽位并启动下一个排队任务
    release_slot();
}

// ---------- 任务调度：最大并发任务数（设置页可配，超出排队） ----------

struct Scheduler {
    running: usize,
    max: usize,
    pending: VecDeque<Box<dyn FnOnce() + Send>>,
}

static SCHEDULER: OnceLock<Mutex<Scheduler>> = OnceLock::new();

fn scheduler() -> &'static Mutex<Scheduler> {
    SCHEDULER.get_or_init(|| {
        Mutex::new(Scheduler {
            running: 0,
            max: easys3_core::settings::DEFAULT_MAX_CONCURRENT_TASKS as usize,
            pending: VecDeque::new(),
        })
    })
}

/// 更新并发上限（启动 / 保存设置时调用；只影响新任务准入，不改排队顺序）
pub fn set_max_concurrent_tasks(max: u32) {
    let mut s = lock(scheduler());
    s.max = max as usize;
    // 上调后可能立即有空位：取出排队任务补位（不占锁执行 starter）
    loop {
        if s.running >= s.max {
            break;
        }
        match s.pending.pop_front() {
            Some(starter) => {
                s.running += 1;
                drop(s);
                starter();
                s = lock(scheduler());
            }
            None => break,
        }
    }
}

/// 按并发上限调度任务；满载时置为 Queued 排队，有任务结束自动启动。
fn schedule(shared: &Arc<TaskShared>, starter: impl FnOnce() + Send + 'static) {
    let mut s = lock(scheduler());
    if s.running < s.max {
        s.running += 1;
        drop(s);
        starter();
        return;
    }
    lock(&shared.info).status = TaskStatus::Queued;
    s.pending.push_back(Box::new(starter));
}

/// 任务真正开始执行（含从排队中唤醒）：Queued → Running
fn mark_running(shared: &Arc<TaskShared>) {
    lock(&shared.info).status = TaskStatus::Running;
}

/// 任务结束后释放槽位；队列非空则按序启动下一个（先占位再执行，防超发）。
fn release_slot() {
    let next = {
        let mut s = lock(scheduler());
        s.running = s.running.saturating_sub(1);
        match s.pending.pop_front() {
            Some(starter) => {
                s.running += 1;
                Some(starter)
            }
            None => None,
        }
    };
    if let Some(starter) = next {
        starter();
    }
}

/// 瞬时错误自动重试的退避间隔：300ms / 600ms / 1200ms …
async fn retry_delay(attempt: usize) {
    tokio::time::sleep(Duration::from_millis(300 * (1u64 << attempt))).await;
}

/// 记录一次自动重试（任务中心可见）并立即推送
fn note_retry(app: &tauri::AppHandle, shared: &Arc<TaskShared>) {
    lock(&shared.info).retry_count += 1;
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

#[allow(clippy::too_many_arguments)]
pub fn spawn_upload(
    app: tauri::AppHandle,
    client: aws_sdk_s3::Client,
    bucket: String,
    files: Vec<(PathBuf, String)>,
    cancel: Arc<AtomicBool>,
    shared: Arc<TaskShared>,
    storage_class: Option<aws_sdk_s3::types::StorageClass>,
    settings: TransferSettings,
    throttle: Arc<Throttle>,
) {
    let probe = shared.clone();
    schedule(&probe, move || {
        tauri::async_runtime::spawn(async move {
            mark_running(&shared);
            let queue = Arc::new(Mutex::new(VecDeque::from(files)));
            let mut workers = Vec::new();
            // 文件间并发（设置页可配，默认 2）
            for _ in 0..settings.file_concurrency.max(1) {
                let q = queue.clone();
                let c = cancel.clone();
                let s = shared.clone();
                let cl = client.clone();
                let b = bucket.clone();
                let ah = app.clone();
                let opts = UploadOptions {
                    storage_class: storage_class.clone(),
                    part_size: settings.part_size_bytes(),
                    throttle: Some(throttle.clone()),
                };
                // 瞬时错误自动重试次数（设置页可配，默认 3；0 = 关闭）
                let auto_retries = settings.auto_retry_count as usize;
                workers.push(tauri::async_runtime::spawn(async move {
                    loop {
                        if c.load(Ordering::Relaxed) {
                            break;
                        }
                        let item = { lock(&q).pop_front() };
                        let Some((path, key)) = item else { break };
                        lock(&s.info).current_file = Some(key.clone());
                        let progress = make_delta_progress(&ah, &s);
                        let mut result = Err(CoreError::Other("上传未执行".into()));
                        for attempt in 0..=auto_retries {
                            result = upload_file(
                                &cl,
                                &b,
                                &key,
                                &path,
                                progress.clone(),
                                c.clone(),
                                opts.clone(),
                            )
                            .await;
                            if result.is_ok()
                                || !result.as_ref().err().is_some_and(CoreError::is_retryable)
                                || attempt == auto_retries
                                || c.load(Ordering::Relaxed)
                            {
                                break;
                            }
                            note_retry(&ah, &s);
                            retry_delay(attempt).await;
                        }
                        match result {
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
    });
}

// ---------- 下载 ----------

// 后台任务入口的参数即跨线程移交的全部任务上下文（客户端快照 / 取消 / 状态 / 重试槽），
// 语义上是平铺的移交清单，强行聚合成结构体只会掩盖一一对应关系
#[allow(clippy::too_many_arguments)]
pub fn spawn_download_expand(
    app: tauri::AppHandle,
    client: aws_sdk_s3::Client,
    control: aws_sdk_s3::Client,
    bucket: String,
    items: Vec<FsItem>,
    dest_dir: PathBuf,
    conflict: String,
    // 重试范围：None = 首次执行；Some = 仅执行失败范围内的对象（重试，规格）
    retry_failed: Option<HashSet<String>>,
    cancel: Arc<AtomicBool>,
    shared: Arc<TaskShared>,
    retry_slot: Arc<Mutex<Option<RetryJob>>>,
    settings: TransferSettings,
    throttle: Arc<Throttle>,
) {
    let probe = shared.clone();
    schedule(&probe, move || {
        tauri::async_runtime::spawn(async move {
            mark_running(&shared);
            // 原始列表项存入重试槽：重试按原参数重新展开（展开失败的文件夹也可重试）
            let items_for_retry = items.clone();
            // 展开：文件夹（前缀）递归列出全部对象，保留目录结构（规格）
            let mut jobs: Vec<DownloadJob> = Vec::new();
            for item in items {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                if item.is_dir {
                    let prefix = item.key.clone();
                    // false = 下载不含目录占位对象（规格 §3）；列出走控制面客户端（带超时）
                    match collect_prefix_objects(&control, &bucket, &prefix, &cancel, false).await {
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
            // 重试：仅保留失败范围内的对象——已成功的不重复下载
            // （rename 冲突策略下重下已成功文件会产生 "name (1)" 副本，必须排除）
            if let Some(failed) = &retry_failed {
                jobs.retain(|j| key_matches_failure(&j.key, failed));
            }
            {
                let mut gi = lock(&shared.info);
                gi.files_total = jobs.len() + gi.files_failed;
            }
            *lock(&retry_slot) = Some(RetryJob::Download {
                client: client.clone(),
                control: control.clone(),
                bucket: bucket.clone(),
                items: items_for_retry,
                dest_dir,
                conflict: conflict.clone(),
            });
            if !cancel.load(Ordering::Relaxed) && !jobs.is_empty() {
                run_download_jobs(
                    &app,
                    &client,
                    &bucket,
                    jobs,
                    &conflict,
                    &cancel,
                    &shared,
                    settings.auto_retry_count as usize,
                    &throttle,
                )
                .await;
            } else {
                finish_task(&app, &cancel, &shared);
            }
        });
    });
}

#[allow(clippy::too_many_arguments)]
async fn run_download_jobs(
    app: &tauri::AppHandle,
    client: &aws_sdk_s3::Client,
    bucket: &str,
    jobs: Vec<DownloadJob>,
    conflict: &str,
    cancel: &Arc<AtomicBool>,
    shared: &Arc<TaskShared>,
    auto_retries: usize,
    throttle: &Arc<Throttle>,
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
        let mut result = Err(CoreError::Other("下载未执行".into()));
        for attempt in 0..=auto_retries {
            result = download_object(
                client,
                bucket,
                &job.key,
                &dest,
                progress.clone(),
                cancel.clone(),
                Some(throttle.clone()),
            )
            .await;
            if result.is_ok()
                || !result.as_ref().err().is_some_and(CoreError::is_retryable)
                || attempt == auto_retries
                || cancel.load(Ordering::Relaxed)
            {
                break;
            }
            note_retry(app, shared);
            retry_delay(attempt).await;
        }
        match result {
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

// 同 spawn_download_expand：平铺的任务上下文移交清单
#[allow(clippy::too_many_arguments)]
pub fn spawn_delete_expand(
    app: tauri::AppHandle,
    client: aws_sdk_s3::Client,
    bucket: String,
    items: Vec<FsItem>,
    // 重试范围：None = 首次执行；Some = 仅删除失败范围内的对象（重试，规格）
    retry_failed: Option<HashSet<String>>,
    cancel: Arc<AtomicBool>,
    shared: Arc<TaskShared>,
    retry_slot: Arc<Mutex<Option<RetryJob>>>,
) {
    // 删除不做自动重试（DeleteObjects 幂等但会重复请求已成功对象，
    // 规格仅要求批量失败保留"手动精确重试"语义）；仅纳入并发任务调度。
    let probe = shared.clone();
    schedule(&probe, move || {
        tauri::async_runtime::spawn(async move {
            mark_running(&shared);
            // 原始列表项存入重试槽：重试按原参数重新展开（展开失败的文件夹也可重试）
            let items_for_retry = items.clone();
            let mut keys: Vec<String> = Vec::new();
            for item in items {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                if item.is_dir {
                    let prefix = item.key.clone();
                    // true = 删除含目录占位对象（随前缀一并删除，规格 §4）
                    match collect_prefix_objects(&client, &bucket, &prefix, &cancel, true).await {
                        Ok(ks) => keys.extend(ks),
                        Err(CoreError::Cancelled) => break,
                        Err(e) => record_failure(&shared, &prefix, format!("列出对象失败：{e}")),
                    }
                } else {
                    keys.push(item.key.clone());
                }
            }
            // 重试：仅保留失败范围内的 key——已成功的不重复删除（删除虽幂等，避免无谓请求）
            if let Some(failed) = &retry_failed {
                keys.retain(|k| key_matches_failure(k, failed));
            }
            {
                let mut gi = lock(&shared.info);
                gi.files_total = keys.len() + gi.files_failed;
            }
            *lock(&retry_slot) = Some(RetryJob::Delete {
                client: client.clone(),
                bucket: bucket.clone(),
                items: items_for_retry,
            });
            if !cancel.load(Ordering::Relaxed) && !keys.is_empty() {
                run_delete_keys(&app, &client, &bucket, keys, &cancel, &shared).await;
            } else {
                finish_task(&app, &cancel, &shared);
            }
        });
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
    // 批量执行：部分成功也如实计入进度；请求级失败的剩余对象
    // 已由 delete_keys 逐个记入失败（真实 key，重试可命中，规格 §4）
    let outcome = delete_keys(client, bucket, &keys, cancel.clone()).await;
    {
        let mut gi = lock(&shared.info);
        gi.files_done += outcome.report.deleted;
        gi.files_failed += outcome.report.failures.len();
        gi.failures
            .extend(outcome.report.failures.into_iter().map(|f| Failure {
                key: f.key,
                error: f.message,
            }));
    }
    finish_task(app, cancel, shared);
}

// ---------- 桶内复制 / 移动 / 重命名 ----------

/// key 的文件名（最后一段）
fn base_name(key: &str) -> &str {
    let trimmed = key.strip_suffix('/').unwrap_or(key);
    trimmed.rsplit('/').next().unwrap_or(trimmed)
}

/// 计算单个对象的复制目标：
/// - 单文件（rename = None）→ `目标前缀 + 原文件名`；
/// - 单对象重命名（rename = Some）→ `目标前缀 + 新名`；
/// - 目录展开的对象 → `目标前缀 + 相对路径`（占位对象 `a/b/` 相对 `a/b/` 为空串
///   时恰好得到 `目标前缀`，即目标目录的占位对象）。
fn copy_target(target_prefix: &str, source_prefix: Option<&str>, key: &str) -> String {
    let rel = match source_prefix {
        Some(prefix) => key.strip_prefix(prefix).unwrap_or(key),
        None => base_name(key),
    };
    format!("{target_prefix}{rel}")
}

// 同 spawn_delete_expand：平铺的任务上下文移交清单
#[allow(clippy::too_many_arguments)]
pub fn spawn_copy(
    app: tauri::AppHandle,
    client: aws_sdk_s3::Client,
    bucket: String,
    items: Vec<FsItem>,
    target_prefix: String,
    conflict: String,
    new_name: Option<String>,
    // true = 移动（复制成功后删除源对象）
    remove: bool,
    // 重试范围：None = 首次执行；Some = 仅执行失败范围内的对象（重试，规格）
    retry_failed: Option<HashSet<String>>,
    cancel: Arc<AtomicBool>,
    shared: Arc<TaskShared>,
    retry_slot: Arc<Mutex<Option<RetryJob>>>,
    settings: TransferSettings,
) {
    let probe = shared.clone();
    schedule(&probe, move || {
        tauri::async_runtime::spawn(async move {
            mark_running(&shared);
            // 原始列表项存入重试槽：重试按原参数重新展开（展开失败的文件夹也可重试）
            let items_for_retry = items.clone();
            let mut jobs: Vec<CopyJob> = Vec::new();
            // 重命名仅对单个文件有意义
            let single_rename = if items.len() == 1 {
                new_name.clone()
            } else {
                None
            };
            for item in items {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                if item.is_dir {
                    let prefix = item.key.clone();
                    // true = 目录操作一并处理 0 字节占位对象（规格 §6）
                    match collect_prefix_objects(&client, &bucket, &prefix, &cancel, true).await {
                        Ok(keys) => {
                            for k in keys {
                                jobs.push(CopyJob {
                                    target: copy_target(&target_prefix, Some(&prefix), &k),
                                    source: k,
                                    is_move: remove,
                                });
                            }
                        }
                        Err(CoreError::Cancelled) => break,
                        Err(e) => record_failure(&shared, &prefix, format!("列出对象失败：{e}")),
                    }
                } else {
                    let target = match single_rename.as_deref() {
                        Some(name) => format!("{target_prefix}{name}"),
                        None => copy_target(&target_prefix, None, &item.key),
                    };
                    jobs.push(CopyJob {
                        source: item.key.clone(),
                        target,
                        is_move: remove,
                    });
                }
            }
            // 重试：仅保留失败范围内的对象——已成功的不重复复制
            if let Some(failed) = &retry_failed {
                jobs.retain(|j| key_matches_failure(&j.source, failed));
            }
            {
                let mut gi = lock(&shared.info);
                gi.files_total = jobs.len() + gi.files_failed;
            }
            *lock(&retry_slot) = Some(RetryJob::Copy {
                client: client.clone(),
                bucket: bucket.clone(),
                items: items_for_retry,
                target_prefix: target_prefix.clone(),
                new_name,
                conflict: conflict.clone(),
                remove,
            });
            if !cancel.load(Ordering::Relaxed) && !jobs.is_empty() {
                run_copy_jobs(
                    &app,
                    &client,
                    &bucket,
                    jobs,
                    &conflict,
                    &cancel,
                    &shared,
                    settings.auto_retry_count as usize,
                )
                .await;
            } else {
                finish_task(&app, &cancel, &shared);
            }
        });
    });
}

#[allow(clippy::too_many_arguments)]
async fn run_copy_jobs(
    app: &tauri::AppHandle,
    client: &aws_sdk_s3::Client,
    bucket: &str,
    jobs: Vec<CopyJob>,
    conflict: &str,
    cancel: &Arc<AtomicBool>,
    shared: &Arc<TaskShared>,
    auto_retries: usize,
) {
    for job in jobs {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        lock(&shared.info).current_file = Some(job.source.clone());
        let mut result = Err(CoreError::Other("复制未执行".into()));
        for attempt in 0..=auto_retries {
            result = easys3_core::object::copy_object(
                client,
                bucket,
                &job.source,
                &job.target,
                job.is_move,
                conflict == "skip",
            )
            .await;
            if result.is_ok()
                || !result.as_ref().err().is_some_and(CoreError::is_retryable)
                || attempt == auto_retries
                || cancel.load(Ordering::Relaxed)
            {
                break;
            }
            note_retry(app, shared);
            retry_delay(attempt).await;
        }
        match result {
            Ok(outcome) => {
                let mut gi = lock(&shared.info);
                match outcome {
                    easys3_core::object::CopyOutcome::Copied => gi.files_done += 1,
                    easys3_core::object::CopyOutcome::Skipped => gi.files_skipped += 1,
                }
            }
            Err(e) => record_failure(shared, &job.source, e.to_string()),
        }
        emit_task(app, shared, false);
    }
    finish_task(app, cancel, shared);
}

// ---------- 下载后打开（P0-10） ----------

/// 下载单个对象到临时目录后用系统默认程序打开。进度经任务中心反馈（不阻塞界面）。
#[allow(clippy::too_many_arguments)]
pub fn spawn_open(
    app: tauri::AppHandle,
    client: aws_sdk_s3::Client,
    bucket: String,
    key: String,
    dest: PathBuf,
    cancel: Arc<AtomicBool>,
    shared: Arc<TaskShared>,
    auto_retries: usize,
    throttle: Arc<Throttle>,
) {
    let probe = shared.clone();
    schedule(&probe, move || {
        tauri::async_runtime::spawn(async move {
            mark_running(&shared);
            if let Some(dir) = dest.parent() {
                let _ = tokio::fs::create_dir_all(dir).await;
            }
            lock(&shared.info).current_file = Some(key.clone());
            let progress = make_growing_progress(&app, &shared);
            let mut result = Err(CoreError::Other("打开未执行".into()));
            for attempt in 0..=auto_retries {
                result = download_object(
                    &client,
                    &bucket,
                    &key,
                    &dest,
                    progress.clone(),
                    cancel.clone(),
                    Some(throttle.clone()),
                )
                .await;
                if result.is_ok()
                    || !result.as_ref().err().is_some_and(CoreError::is_retryable)
                    || attempt == auto_retries
                    || cancel.load(Ordering::Relaxed)
                {
                    break;
                }
                note_retry(&app, &shared);
                retry_delay(attempt).await;
            }
            match result {
                Ok(()) => {
                    lock(&shared.info).files_done += 1;
                    use tauri_plugin_opener::OpenerExt;
                    if let Err(e) = app
                        .opener()
                        .open_path(dest.to_string_lossy().to_string(), None::<String>)
                    {
                        record_failure(&shared, &key, format!("启动默认程序失败：{e}"));
                    }
                }
                Err(CoreError::Cancelled) => {}
                Err(e) => record_failure(&shared, &key, e.to_string()),
            }
            finish_task(&app, &cancel, &shared);
        });
    });
}

#[cfg(test)]
mod copy_tests {
    use super::*;

    #[test]
    fn copy_target_maps_files_and_dirs() {
        // 单文件：目标前缀 + 原名（中文 / 空格原样保留）
        assert_eq!(copy_target("x/", None, "a/中文 名.txt"), "x/中文 名.txt");
        // 单对象重命名
        assert_eq!(copy_target("a/", None, "a/old.txt"), "a/old.txt");
        // 目录展开：保持相对层级；占位对象相对路径为空 → 目标前缀本身
        assert_eq!(copy_target("x/", Some("a/b/"), "a/b/c/d.txt"), "x/c/d.txt");
        assert_eq!(copy_target("x/", Some("a/b/"), "a/b/"), "x/");
        assert_eq!(base_name("a/b/c.txt"), "c.txt");
        assert_eq!(base_name("a/b/占位/"), "占位");
    }
}
