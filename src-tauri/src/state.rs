//! 应用状态：配置文件、按连接缓存的 S3 客户端、任务注册表。
//!
//! 锁纪律：std::sync::Mutex 只在同步临界区内短暂持有，绝不跨 .await；
//! 异步命令先克隆 Client（内部 Arc，克隆廉价）再释放锁执行 S3 操作。

use crate::secrets::SecretStore;
use crate::tasks::TaskEntry;
use easys3_core::config::{load_connections, save_connections, ConnectionConfig, ConnectionsFile};
use easys3_core::settings::{load_settings, TransferSettings};
use easys3_core::throttle::Throttle;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use tauri::Manager;

pub struct App {
    pub config_path: PathBuf,
    pub secrets: SecretStore,
    pub settings_path: PathBuf,
    /// 传输设置快照（保存设置时更新；任务启动时读取快照）
    pub settings: Mutex<TransferSettings>,
    /// 全局上传限速（跨任务共享，保存设置热调整）
    pub upload_throttle: Arc<Throttle>,
    /// 全局下载限速（跨任务共享，保存设置热调整）
    pub download_throttle: Arc<Throttle>,
    pub inner: Mutex<Inner>,
}

pub struct Inner {
    pub file: ConnectionsFile,
    /// 传输客户端缓存（上传 / 下载对象体，无操作超时）
    pub clients: HashMap<String, aws_sdk_s3::Client>,
    /// 控制面客户端缓存（浏览 / 测试 / 计数 / 预览 / 删除，30s 尝试超时，规格）
    pub control_clients: HashMap<String, aws_sdk_s3::Client>,
    pub tasks: HashMap<String, TaskEntry>,
}

/// 防毒化：任一线程 panic 后仍能恢复数据
pub fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// "打开"操作的临时目录（系统临时目录下），退出时清理（P0-10）。
pub fn open_temp_dir() -> PathBuf {
    std::env::temp_dir().join("easys3-open")
}

impl App {
    pub fn init(handle: &tauri::AppHandle) -> Self {
        let dir = handle
            .path()
            .app_config_dir()
            .expect("无法确定应用配置目录");
        let config_path = dir.join("connections.json");
        let secrets = SecretStore::new(dir.clone());
        let mut file = load_connections(&config_path).unwrap_or_default();
        // 一次性迁移历史明文 Secret Key；迁移成功后配置文件不再保留密钥。
        let mut migrated = false;
        for connection in &mut file.connections {
            if !connection.secret_key.is_empty()
                && secrets.set(&connection.id, &connection.secret_key).is_ok()
            {
                connection.secret_key.clear();
                migrated = true;
            }
        }
        if migrated {
            let _ = save_connections(&config_path, &file);
        }
        let settings_path = dir.join("settings.json");
        let settings = load_settings(&settings_path);
        let upload_throttle = Arc::new(Throttle::new(settings.upload_limit_kbps * 1024));
        let download_throttle = Arc::new(Throttle::new(settings.download_limit_kbps * 1024));
        crate::tasks::set_max_concurrent_tasks(settings.max_concurrent_tasks);
        App {
            config_path,
            secrets,
            settings_path,
            settings: Mutex::new(settings),
            upload_throttle,
            download_throttle,
            inner: Mutex::new(Inner {
                file,
                clients: HashMap::new(),
                control_clients: HashMap::new(),
                tasks: HashMap::new(),
            }),
        }
    }

    /// 当前设置快照（任务启动 / 重试时读取）
    pub fn transfer_settings(&self) -> TransferSettings {
        lock(&self.settings).clone()
    }

    pub fn current_connection(&self) -> Result<ConnectionConfig, String> {
        let inner = lock(&self.inner);
        let id = inner
            .file
            .current_connection_id
            .clone()
            .ok_or("尚未选择连接，请先在左侧新建或选择连接")?;
        let mut connection = inner
            .file
            .connections
            .clone()
            .into_iter()
            .find(|p| p.id == id)
            .ok_or_else(|| "连接不存在".to_string())?;
        drop(inner);
        connection.secret_key = self
            .secrets
            .get(&connection.id)?
            .ok_or("未找到该连接的 Secret Key，请在连接设置中重新填写")?;
        Ok(connection)
    }

    /// 当前连接的 S3 客户端（按连接缓存；配置变更时由命令层主动失效）。
    pub fn current_client(&self) -> Result<(aws_sdk_s3::Client, String), String> {
        let connection = self.current_connection()?;
        let client = {
            let mut inner = lock(&self.inner);
            if let Some(c) = inner.clients.get(&connection.id) {
                c.clone()
            } else {
                let c = easys3_core::s3::build_client(&connection).map_err(|e| e.to_string())?;
                inner.clients.insert(connection.id.clone(), c.clone());
                c
            }
        };
        Ok((client, connection.name))
    }

    /// 当前连接的控制面客户端（浏览 / 测试 / 计数 / 预览 / 删除：30s 尝试超时，规格）。
    pub fn current_control_client(&self) -> Result<(aws_sdk_s3::Client, String), String> {
        let connection = self.current_connection()?;
        let client = {
            let mut inner = lock(&self.inner);
            if let Some(c) = inner.control_clients.get(&connection.id) {
                c.clone()
            } else {
                let c = easys3_core::s3::build_control_client(&connection)
                    .map_err(|e| e.to_string())?;
                inner
                    .control_clients
                    .insert(connection.id.clone(), c.clone());
                c
            }
        };
        Ok((client, connection.name))
    }

    pub fn save_file(&self) -> Result<(), String> {
        let inner = lock(&self.inner);
        save_connections(&self.config_path, &inner.file).map_err(|e| e.to_string())
    }
}
