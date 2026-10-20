//! 日志、配置导入导出、目录打开、退出等系统命令。

use std::sync::Arc;

use tauri::{AppHandle, State};

use crate::config;
use crate::model::{AppConfig, LogEntry, LogLevel};
use crate::state::AppState;

#[tauri::command]
pub fn get_logs(state: State<'_, Arc<AppState>>, limit: Option<usize>) -> Vec<LogEntry> {
    state.logs(limit.unwrap_or(500))
}

#[tauri::command]
pub fn clear_logs(app: AppHandle, state: State<'_, Arc<AppState>>) {
    let state = state.inner().clone();
    state.clear_logs();
    state.log(&app, LogLevel::Info, "log", "日志已清空", None);
}

#[tauri::command]
pub fn export_logs(
    state: State<'_, Arc<AppState>>,
    path: String,
) -> Result<(), String> {
    let logs = state.logs(usize::MAX);
    let mut text = String::new();
    for entry in logs {
        text.push_str(&format!(
            "{} [{:?}] [{}] {}{}\n",
            entry.ts,
            entry.level,
            entry.scope,
            entry.message,
            entry
                .detail
                .map(|d| format!(" :: {d}"))
                .unwrap_or_default()
        ));
    }
    config::write_atomic(std::path::Path::new(&path), text.as_bytes())
        .map_err(|err| format!("导出日志失败：{err}"))
}

#[tauri::command]
pub fn export_config(
    state: State<'_, Arc<AppState>>,
    path: String,
) -> Result<(), String> {
    let config = state.config();
    config::export_to(std::path::Path::new(&path), &config)
        .map_err(|err| format!("导出配置失败：{err}"))
}

#[tauri::command]
pub fn import_config(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    path: String,
) -> Result<AppConfig, String> {
    let state = state.inner().clone();
    let text = std::fs::read_to_string(&path).map_err(|err| format!("读取配置失败：{err}"))?;
    let mut config: AppConfig =
        serde_json::from_str(&text).map_err(|err| format!("配置格式无效：{err}"))?;
    config.version = crate::model::CONFIG_VERSION;
    let snapshot = config.clone();
    state
        .mutate_config(|current| *current = snapshot.clone())
        .map_err(|err| format!("保存配置失败：{err}"))?;
    state.autostart_cache_invalidate();
    state.log(
        &app,
        LogLevel::Success,
        "config",
        "已导入配置",
        Some(path),
    );
    Ok(state.config())
}

#[tauri::command]
pub fn open_config_dir() -> Result<(), String> {
    open_in_explorer(&config::data_dir())
}

#[tauri::command]
pub fn open_log_dir() -> Result<(), String> {
    open_in_explorer(&config::log_dir())
}

fn open_in_explorer(path: &std::path::Path) -> Result<(), String> {
    std::fs::create_dir_all(path).map_err(|err| format!("创建目录失败：{err}"))?;
    std::process::Command::new("explorer")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|err| format!("打开目录失败：{err}"))
}

#[tauri::command]
pub fn quit_app(app: AppHandle) {
    app.exit(0);
}
