//! 配置与设置相关命令。

use std::sync::Arc;

use tauri::{AppHandle, State};

use crate::model::{AppConfig, AppSettings, LogLevel, SystemInfo};
use crate::platform;
use crate::state::AppState;

#[tauri::command]
pub fn get_app_config(state: State<'_, Arc<AppState>>) -> AppConfig {
    state.config()
}

#[tauri::command]
pub fn save_settings(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    settings: AppSettings,
) -> Result<AppConfig, String> {
    let state = state.inner().clone();
    let mut settings = settings;

    // 基本校验，避免前端传入非法值。
    if !matches!(settings.language.as_str(), "zh-CN" | "en") {
        settings.language = "zh-CN".into();
    }
    if !matches!(settings.theme.as_str(), "system" | "light" | "dark") {
        settings.theme = "system".into();
    }
    settings.max_concurrency = settings.max_concurrency.clamp(1, 32);
    settings.max_log_entries = settings.max_log_entries.clamp(200, 20_000);
    settings.max_run_history = settings.max_run_history.clamp(10, 500);
    settings.undo_limit = settings.undo_limit.clamp(1, 10_000);
    settings.startup_grace_ms = settings.startup_grace_ms.min(600_000);
    settings.default_node_timeout_ms = settings.default_node_timeout_ms.min(3_600_000);

    let previous = state.config();
    if previous.settings.launch_at_login != settings.launch_at_login {
        platform::set_self_autostart(settings.launch_at_login)
            .map_err(|err| format!("设置 DaemonMan 开机自启失败：{err}"))?;
        state.log(
            &app,
            LogLevel::Info,
            "settings",
            format!(
                "DaemonMan 开机自启已{}",
                if settings.launch_at_login {
                    "启用"
                } else {
                    "关闭"
                }
            ),
            None,
        );
    }

    state
        .mutate_config(|config| {
            config.settings = settings.clone();
        })
        .map_err(|err| format!("保存设置失败：{err}"))?;

    Ok(state.config())
}

#[tauri::command]
pub fn get_system_info(state: State<'_, Arc<AppState>>) -> SystemInfo {
    let config = state.config();
    let items = state.autostart_cached(false);
    let disabled = items.iter().filter(|i| !i.enabled).count();

    SystemInfo {
        os: "Windows".to_string(),
        arch: std::env::consts::ARCH.to_string(),
        hostname: std::env::var("COMPUTERNAME").unwrap_or_else(|_| "unknown".into()),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        config_path: state.config_path.display().to_string(),
        log_path: state.log_path.display().to_string(),
        elevated: platform::is_elevated(),
        autostart_total: items.len(),
        autostart_disabled: disabled,
        workflow_count: config.workflows.len(),
        running_workflows: state.running_run_count(),
        uptime_secs: state.uptime_secs(),
    }
}

#[tauri::command]
pub fn set_self_autostart(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    enabled: bool,
) -> Result<bool, String> {
    let state = state.inner().clone();
    platform::set_self_autostart(enabled).map_err(|err| err.to_string())?;
    state
        .mutate_config(|config| {
            config.settings.launch_at_login = enabled;
        })
        .map_err(|err| err.to_string())?;
    state.log(
        &app,
        LogLevel::Info,
        "settings",
        format!(
            "DaemonMan 开机自启已{}",
            if enabled { "启用" } else { "关闭" }
        ),
        None,
    );
    Ok(enabled)
}

#[tauri::command]
pub fn get_self_autostart() -> bool {
    platform::is_self_autostart_enabled()
}
