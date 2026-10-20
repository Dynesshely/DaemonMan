//! DaemonMan 后端入口。

mod commands;
mod config;
mod engine;
mod legacy;
mod model;
mod platform;
mod state;
mod tray;

use std::sync::Arc;
use std::time::Duration;

use tauri::{Emitter, Manager, WindowEvent};

use crate::model::{LogLevel, RunTrigger};
use crate::state::AppState;

/// 从旧版 ProgressManager 配置导入工作流（无窗口 CLI）。
/// 用法：`DaemonMan.exe --import-legacy <config.json 路径>`。
fn run_import_legacy(path: &str) -> i32 {
    let converted = match legacy::convert_file(std::path::Path::new(path)) {
        Ok(converted) => converted,
        Err(error) => {
            eprintln!("导入失败：{error:#}");
            return 1;
        }
    };

    let workflow = converted.workflow;
    let nodes = workflow.nodes.len();
    let edges = workflow.edges.len();
    let name = workflow.name.clone();
    let warnings = converted.warnings;

    let mut app_config = config::load_or_init();
    let replaced = legacy::import_into_config(&mut app_config, workflow);
    if let Err(error) = config::save(&app_config) {
        eprintln!("保存配置失败：{error:#}");
        return 1;
    }

    println!("config: {}", config::config_path().display());
    println!("workflow: {name}");
    println!("nodes: {nodes}");
    println!("edges: {edges}");
    println!("replaced: {replaced}");
    for warning in &warnings {
        println!("warning: {warning}");
    }
    println!("import: ok");
    0
}

fn import_legacy_arg(args: &[String]) -> Option<String> {
    if let Some(index) = args.iter().position(|arg| arg == "--import-legacy") {
        return args.get(index + 1).cloned();
    }
    args.iter()
        .find_map(|arg| arg.strip_prefix("--import-legacy=").map(str::to_string))
}

/// 无窗口自检：验证配置、平台层扫描与进程枚举是否可用。
/// 用法：`DaemonMan.exe --selftest`（退出码 0 表示自检通过）。
fn run_selftest() {
    let state = AppState::new();
    println!("config: {}", state.config_path.display());
    println!("elevated: {}", platform::is_elevated());
    println!("self_autostart: {}", platform::is_self_autostart_enabled());

    let items = platform::scan_autostart();
    println!("autostart_total: {}", items.len());
    let mut by_source: std::collections::BTreeMap<String, usize> = Default::default();
    for item in &items {
        *by_source.entry(item.source_label.clone()).or_default() += 1;
    }
    for (source, count) in by_source {
        println!("autostart[{source}]: {count}");
    }
    for item in items.iter().filter(|i| !i.enabled).take(5) {
        println!("disabled_item: {} ({})", item.name, item.id);
    }

    let processes = platform::list_processes();
    println!("processes: {}", processes.len());

    let config = state.config();
    println!("workflows: {}", config.workflows.len());
    for workflow in &config.workflows {
        let validation = engine::validate(workflow);
        println!(
            "workflow: name={:?} nodes={} edges={} enabled={} valid={}",
            workflow.name,
            workflow.nodes.len(),
            workflow.edges.len(),
            workflow.enabled,
            validation.ok
        );
        for error in &validation.errors {
            println!("  error: {error}");
        }
    }
    println!("selftest: ok");
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let args: Vec<String> = std::env::args().collect();

    if args.iter().any(|arg| arg == "--selftest") {
        run_selftest();
        return;
    }

    if args
        .iter()
        .any(|arg| arg == "--import-legacy" || arg.starts_with("--import-legacy="))
    {
        match import_legacy_arg(&args) {
            Some(path) => std::process::exit(run_import_legacy(&path)),
            None => {
                eprintln!("用法：DaemonMan.exe --import-legacy <旧版 config.json 路径>");
                std::process::exit(2);
            }
        }
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            tray::show_main(app);
            let state = app.state::<Arc<AppState>>().inner().clone();
            state.log(
                app,
                LogLevel::Info,
                "core",
                "检测到另一个 DaemonMan 实例，已切换到已有窗口",
                None,
            );
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(
            // 不持久化 VISIBLE：否则「登录启动 + 最小化到托盘」会被上一次的可见状态覆盖。
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(
                    tauri_plugin_window_state::StateFlags::SIZE
                        | tauri_plugin_window_state::StateFlags::POSITION
                        | tauri_plugin_window_state::StateFlags::MAXIMIZED
                        | tauri_plugin_window_state::StateFlags::DECORATIONS
                        | tauri_plugin_window_state::StateFlags::FULLSCREEN,
                )
                .build(),
        )
        .setup(|app| {
            let state = Arc::new(AppState::new());
            app.manage(state.clone());

            let handle = app.handle().clone();
            tray::setup(&handle)?;

            let config = state.config();
            state.log(
                &handle,
                LogLevel::Info,
                "core",
                format!(
                    "DaemonMan v{} 已启动（配置：{}）",
                    env!("CARGO_PKG_VERSION"),
                    state.config_path.display()
                ),
                None,
            );

            // 保证配置里的开机自启与系统实际状态一致。
            let actual = platform::is_self_autostart_enabled();
            if config.settings.launch_at_login != actual {
                if let Err(err) = platform::set_self_autostart(config.settings.launch_at_login) {
                    state.log(
                        &handle,
                        LogLevel::Warn,
                        "settings",
                        "同步 DaemonMan 开机自启状态失败",
                        Some(err.to_string()),
                    );
                }
            }

            let launched_by_autostart = std::env::args().any(|arg| arg == "--autostart");
            // 窗口在配置里是 visible=false，由这里决定是否显示：
            // - 普通启动：立即显示；
            // - 登录自启：默认隐藏到托盘，除非用户关闭了「启动时最小化到托盘」。
            let should_show = !launched_by_autostart || !config.settings.start_minimized;
            if let Some(window) = app.get_webview_window("main") {
                if should_show {
                    let _ = window.show();
                    let _ = window.set_focus();
                } else {
                    let _ = window.hide();
                }
                state.log(
                    &handle,
                    LogLevel::Debug,
                    "startup",
                    format!(
                        "窗口初始化：autostart={launched_by_autostart} start_minimized={} should_show={should_show}",
                        config.settings.start_minimized
                    ),
                    None,
                );
            }

            if launched_by_autostart && config.settings.run_workflows_on_start {
                let grace = config.settings.startup_grace_ms;
                let app_handle = handle.clone();
                let state_handle = state.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(grace)).await;
                    state_handle.log(
                        &app_handle,
                        LogLevel::Info,
                        "startup",
                        format!("登录启动：开始执行启用中的工作流（等待 {grace}ms）"),
                        None,
                    );
                    commands::workflow::run_all_enabled_impl(
                        &app_handle,
                        &state_handle,
                        RunTrigger::Startup,
                    );
                });
            }

            // 通知前端当前状态（前端可能在 setup 之后才挂载）。
            let _ = handle.emit("app:ready", true);

            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let app = window.app_handle();
                let state = app.state::<Arc<AppState>>();
                if state.config().settings.minimize_to_tray {
                    api.prevent_close();
                    let _ = window.hide();
                    state.log(app, LogLevel::Debug, "window", "主窗口已隐藏到托盘", None);
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::config::get_app_config,
            commands::config::save_settings,
            commands::config::get_system_info,
            commands::config::set_self_autostart,
            commands::config::get_self_autostart,
            commands::workflow::list_workflows,
            commands::workflow::save_workflow,
            commands::workflow::delete_workflow,
            commands::workflow::duplicate_workflow,
            commands::workflow::validate_workflow,
            commands::workflow::run_workflow,
            commands::workflow::run_all_enabled,
            commands::workflow::cancel_run,
            commands::workflow::get_recent_runs,
            commands::workflow::test_launch,
            commands::autostart::scan_autostart,
            commands::autostart::set_autostart_enabled,
            commands::autostart::take_over_autostart,
            commands::autostart::release_autostart,
            commands::autostart::disable_autostart_item,
            commands::autostart::restore_autostart_item,
            commands::autostart::delete_autostart_item,
            commands::autostart::reveal_autostart_item,
            commands::autostart::read_autostart_script,
            commands::autostart::write_autostart_script,
            commands::process::list_processes,
            commands::process::kill_process,
            commands::system::get_logs,
            commands::system::clear_logs,
            commands::system::export_logs,
            commands::system::export_config,
            commands::system::import_config,
            commands::system::import_legacy_config,
            commands::system::get_file_icons,
            commands::system::open_config_dir,
            commands::system::open_log_dir,
            commands::system::quit_app,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
