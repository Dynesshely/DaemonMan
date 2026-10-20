//! 系统托盘。

use std::sync::Arc;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager};

use crate::commands::workflow::run_all_enabled_impl;
use crate::model::{LogLevel, RunTrigger};
use crate::state::AppState;

pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "打开主界面", true, None::<&str>)?;
    let run_all = MenuItem::with_id(app, "run_all", "运行全部启用工作流", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "设置", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "退出 DaemonMan", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &run_all, &settings, &separator, &quit])?;

    let mut builder = TrayIconBuilder::with_id("daemon-man-tray")
        .tooltip("DaemonMan")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_main(app),
            "settings" => {
                show_main(app);
                let _ = app.emit("navigate", "settings");
            }
            "run_all" => {
                let state = app.state::<Arc<AppState>>().inner().clone();
                state.log(
                    app,
                    LogLevel::Info,
                    "tray",
                    "从托盘运行全部启用工作流",
                    None,
                );
                run_all_enabled_impl(app, &state, RunTrigger::Tray);
            }
            "quit" => {
                let state = app.state::<Arc<AppState>>().inner().clone();
                state.log(app, LogLevel::Info, "tray", "DaemonMan 退出", None);
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| match event {
            TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            }
            | TrayIconEvent::DoubleClick {
                button: MouseButton::Left,
                ..
            } => show_main(tray.app_handle()),
            _ => {}
        });

    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    }

    builder.build(app)?;
    Ok(())
}

pub fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}
