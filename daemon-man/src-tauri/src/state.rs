//! 进程内共享状态。

use std::collections::{HashMap, VecDeque};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;

use anyhow::Result;
use tauri::{AppHandle, Emitter};

use crate::config;
use crate::model::{
    now_iso, AppConfig, AutostartItem, LogEntry, LogLevel, RunStatus, WorkflowRun,
};

pub struct AppState {
    config: RwLock<AppConfig>,
    pub config_path: PathBuf,
    pub log_path: PathBuf,
    logs: Mutex<VecDeque<LogEntry>>,
    log_seq: AtomicU64,
    runs: Mutex<VecDeque<WorkflowRun>>,
    run_flags: Mutex<HashMap<String, Arc<AtomicBool>>>,
    autostart_cache: Mutex<Option<(Instant, Vec<AutostartItem>)>>,
    started: Instant,
}

impl AppState {
    pub fn new() -> Self {
        let _ = config::ensure_dirs();
        let config_exists = config::config_path().exists();
        let config = config::load_or_init();
        if !config_exists {
            // 首次运行落盘默认配置，保证配置/日志目录结构可见。
            let _ = config::save(&config);
        }
        let max_logs = config.settings.max_log_entries.max(100);
        let mut logs = VecDeque::with_capacity(max_logs.min(4096));
        logs.push_back(LogEntry {
            id: 1,
            ts: now_iso(),
            level: LogLevel::Info,
            scope: "core".into(),
            message: "DaemonMan 已启动".into(),
            detail: None,
        });
        Self {
            config: RwLock::new(config),
            config_path: config::config_path(),
            log_path: config::log_path(),
            logs: Mutex::new(logs),
            log_seq: AtomicU64::new(1),
            runs: Mutex::new(VecDeque::new()),
            run_flags: Mutex::new(HashMap::new()),
            autostart_cache: Mutex::new(None),
            started: Instant::now(),
        }
    }

    // -- 配置 ---------------------------------------------------------------

    pub fn config(&self) -> AppConfig {
        self.config.read().expect("config lock poisoned").clone()
    }

    /// 在写锁内修改配置并立即持久化。
    pub fn mutate_config<T>(
        &self,
        f: impl FnOnce(&mut AppConfig) -> T,
    ) -> Result<T> {
        let mut guard = self.config.write().expect("config lock poisoned");
        let result = f(&mut guard);
        let snapshot = guard.clone();
        drop(guard);
        config::save(&snapshot)?;
        Ok(result)
    }

    pub fn uptime_secs(&self) -> u64 {
        self.started.elapsed().as_secs()
    }

    // -- 自启动项缓存 -------------------------------------------------------

    /// 缓存 5 秒内的扫描结果，避免概览页反复触发昂贵的注册表/服务查询。
    pub fn autostart_cached(&self, force: bool) -> Vec<AutostartItem> {
        if !force {
            if let Some((at, items)) = self
                .autostart_cache
                .lock()
                .expect("autostart cache lock poisoned")
                .as_ref()
            {
                if at.elapsed() < std::time::Duration::from_secs(5) {
                    return items.clone();
                }
            }
        }
        let items = crate::platform::scan_autostart();
        *self
            .autostart_cache
            .lock()
            .expect("autostart cache lock poisoned") = Some((Instant::now(), items.clone()));
        items
    }

    pub fn autostart_cache_invalidate(&self) {
        *self
            .autostart_cache
            .lock()
            .expect("autostart cache lock poisoned") = None;
    }

    // -- 日志 ---------------------------------------------------------------

    pub fn logs(&self, limit: usize) -> Vec<LogEntry> {
        let guard = self.logs.lock().expect("log lock poisoned");
        let skip = guard.len().saturating_sub(limit);
        guard.iter().skip(skip).cloned().collect()
    }

    pub fn clear_logs(&self) {
        self.logs.lock().expect("log lock poisoned").clear();
    }

    pub fn log(
        &self,
        app: &AppHandle,
        level: LogLevel,
        scope: &str,
        message: impl Into<String>,
        detail: Option<String>,
    ) {
        let id = self.log_seq.fetch_add(1, Ordering::SeqCst) + 1;
        let entry = LogEntry {
            id,
            ts: now_iso(),
            level,
            scope: scope.to_string(),
            message: message.into(),
            detail: detail.clone(),
        };

        {
            let mut guard = self.logs.lock().expect("log lock poisoned");
            guard.push_back(entry.clone());
            loop {
                let max = self
                    .config
                    .read()
                    .expect("config lock poisoned")
                    .settings
                    .max_log_entries
                    .max(100);
                if guard.len() <= max {
                    break;
                }
                guard.pop_front();
            }
        }

        self.append_log_file(&entry);
        let _ = app.emit("log:entry", &entry);
    }

    fn append_log_file(&self, entry: &LogEntry) {
        if let Ok(meta) = std::fs::metadata(&self.log_path) {
            if meta.len() > 4 * 1024 * 1024 {
                let rotated = self.log_path.with_extension("log.1");
                let _ = std::fs::rename(&self.log_path, rotated);
            }
        }
        let line = format!(
            "{} [{:?}] [{}] {}{}\n",
            entry.ts,
            entry.level,
            entry.scope,
            entry.message,
            entry
                .detail
                .as_ref()
                .map(|d| format!(" :: {d}"))
                .unwrap_or_default()
        );
        if let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)
        {
            let _ = file.write_all(line.as_bytes());
        }
    }

    // -- 运行记录 -----------------------------------------------------------

    pub fn runs(&self) -> Vec<WorkflowRun> {
        self.runs
            .lock()
            .expect("run lock poisoned")
            .iter()
            .cloned()
            .collect()
    }

    pub fn run(&self, run_id: &str) -> Option<WorkflowRun> {
        self.runs
            .lock()
            .expect("run lock poisoned")
            .iter()
            .find(|r| r.run_id == run_id)
            .cloned()
    }

    pub fn upsert_run(&self, run: WorkflowRun) {
        let max = self
            .config
            .read()
            .expect("config lock poisoned")
            .settings
            .max_run_history
            .max(10);
        let mut guard = self.runs.lock().expect("run lock poisoned");
        if let Some(existing) = guard.iter_mut().find(|r| r.run_id == run.run_id) {
            *existing = run;
        } else {
            guard.push_front(run);
        }
        while guard.len() > max {
            guard.pop_back();
        }
    }

    pub fn running_run_count(&self) -> usize {
        self.runs
            .lock()
            .expect("run lock poisoned")
            .iter()
            .filter(|r| r.status == RunStatus::Running)
            .count()
    }

    // -- 取消标记 -----------------------------------------------------------

    pub fn register_run_flag(&self, run_id: &str) -> Arc<AtomicBool> {
        let flag = Arc::new(AtomicBool::new(false));
        self.run_flags
            .lock()
            .expect("flag lock poisoned")
            .insert(run_id.to_string(), flag.clone());
        flag
    }

    pub fn run_flag(&self, run_id: &str) -> Option<Arc<AtomicBool>> {
        self.run_flags
            .lock()
            .expect("flag lock poisoned")
            .get(run_id)
            .cloned()
    }

    pub fn request_cancel(&self, run_id: &str) -> bool {
        if let Some(flag) = self.run_flag(run_id) {
            flag.store(true, Ordering::SeqCst);
            true
        } else {
            false
        }
    }

    pub fn release_run_flag(&self, run_id: &str) {
        self.run_flags
            .lock()
            .expect("flag lock poisoned")
            .remove(run_id);
    }
}
