//! 应用状态：配置文件、按项目缓存的 S3 客户端、任务注册表。
//!
//! 锁纪律：std::sync::Mutex 只在同步临界区内短暂持有，绝不跨 .await；
//! 异步命令先克隆 Client（内部 Arc，克隆廉价）再释放锁执行 S3 操作。

use crate::tasks::TaskEntry;
use easys3_core::config::{load_projects, save_projects, ProjectConfig, ProjectsFile};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};
use tauri::Manager;

pub struct App {
    pub config_path: PathBuf,
    pub inner: Mutex<Inner>,
}

pub struct Inner {
    pub file: ProjectsFile,
    pub clients: HashMap<String, aws_sdk_s3::Client>,
    pub tasks: HashMap<String, TaskEntry>,
}

/// 防毒化：任一线程 panic 后仍能恢复数据
pub fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl App {
    pub fn init(handle: &tauri::AppHandle) -> Self {
        let dir = handle
            .path()
            .app_config_dir()
            .expect("无法确定应用配置目录");
        let config_path = dir.join("projects.json");
        let file = load_projects(&config_path).unwrap_or_default();
        App {
            config_path,
            inner: Mutex::new(Inner {
                file,
                clients: HashMap::new(),
                tasks: HashMap::new(),
            }),
        }
    }

    pub fn current_project(&self) -> Result<ProjectConfig, String> {
        let inner = lock(&self.inner);
        let id = inner
            .file
            .current_project_id
            .clone()
            .ok_or("尚未选择项目，请先在左侧新建或选择项目")?;
        inner
            .file
            .projects
            .clone()
            .into_iter()
            .find(|p| p.id == id)
            .ok_or_else(|| "项目不存在".to_string())
    }

    /// 当前项目的 S3 客户端（按项目缓存；配置变更时由命令层主动失效）。
    pub fn current_client(&self) -> Result<(aws_sdk_s3::Client, String), String> {
        let project = self.current_project()?;
        let client = {
            let mut inner = lock(&self.inner);
            if let Some(c) = inner.clients.get(&project.id) {
                c.clone()
            } else {
                let c = easys3_core::s3::build_client(&project).map_err(|e| e.to_string())?;
                inner.clients.insert(project.id.clone(), c.clone());
                c
            }
        };
        Ok((client, project.name))
    }

    pub fn save_file(&self) -> Result<(), String> {
        let inner = lock(&self.inner);
        save_projects(&self.config_path, &inner.file).map_err(|e| e.to_string())
    }
}
