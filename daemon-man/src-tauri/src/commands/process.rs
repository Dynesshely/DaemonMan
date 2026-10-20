//! 进程查看与结束命令。

use std::collections::HashMap;
use std::sync::Arc;

use tauri::{AppHandle, State};

use crate::model::{LogLevel, ProcessInfo, RunStatus};
use crate::platform;
use crate::state::AppState;

#[tauri::command]
pub fn list_processes(state: State<'_, Arc<AppState>>) -> Vec<ProcessInfo> {
    let mut processes = platform::list_processes();

    let mut managed: HashMap<u32, String> = HashMap::new();
    for run in state.runs() {
        if run.status != RunStatus::Running {
            continue;
        }
        for node in run.nodes.values() {
            if let Some(pid) = node.pid {
                managed.insert(pid, run.workflow_name.clone());
            }
        }
    }

    for process in processes.iter_mut() {
        if let Some(name) = managed.get(&process.pid) {
            process.managed = true;
            process.workflow_name = Some(name.clone());
        }
    }

    processes.sort_by(|a, b| {
        b.managed
            .cmp(&a.managed)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    processes
}

#[tauri::command]
pub fn kill_process(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    pid: u32,
) -> Result<(), String> {
    let state = state.inner().clone();
    match platform::kill_process(pid) {
        Ok(()) => {
            state.log(
                &app,
                LogLevel::Info,
                "process",
                format!("已结束进程 {pid}"),
                None,
            );
            Ok(())
        }
        Err(err) => {
            state.log(
                &app,
                LogLevel::Error,
                "process",
                format!("结束进程 {pid} 失败"),
                Some(err.to_string()),
            );
            Err(err.to_string())
        }
    }
}
