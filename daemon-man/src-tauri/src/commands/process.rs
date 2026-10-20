//! 进程查看与结束命令。

use std::collections::HashMap;
use std::sync::Arc;

use tauri::{AppHandle, State};

use crate::model::{LogLevel, NodeKind, ProcessInfo, RunStatus, TrackedProcess};
use crate::platform;
use crate::state::AppState;

fn process_stem(name: &str) -> String {
    let lower = name.trim().to_lowercase();
    lower.strip_suffix(".exe").unwrap_or(&lower).to_string()
}

fn is_browser_target(target: &str) -> bool {
    let stem = std::path::Path::new(target)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(|stem| stem.to_lowercase())
        .unwrap_or_default();
    matches!(
        stem.as_str(),
        "chrome" | "msedge" | "brave" | "chromium" | "opera" | "vivaldi"
    )
}

/// 按工作流节点匹配当前运行中的进程（包含不是 DaemonMan 启动的进程）。
///
/// 匹配方式：
/// - application 节点的 `processName` 或解析后目标 exe 名；
/// - `.lnk` 先解析成真实目标路径再比对外置可执行文件路径；
/// - 浏览器目标（Chrome/Edge 等）改用 `find_running_app`（窗口标题识别 PWA）；
/// - 运行记录里出现过的 pid 直接纳入（即使当前不是 running 状态）。
#[tauri::command]
pub fn track_workflow_processes(
    state: State<'_, Arc<AppState>>,
    workflow_id: Option<String>,
) -> Vec<TrackedProcess> {
    let config = state.config();
    let processes = platform::list_processes();
    let runs = state.runs();

    // 运行记录托管过的 pid（任何状态的 run 都算，只要进程还活着）。
    let mut managed: HashMap<u32, String> = HashMap::new();
    let mut recorded: HashMap<(String, String), Vec<u32>> = HashMap::new();
    for run in &runs {
        for (node_id, node_state) in &run.nodes {
            if let Some(pid) = node_state.pid {
                managed
                    .entry(pid)
                    .or_insert_with(|| run.workflow_name.clone());
                recorded
                    .entry((run.workflow_id.clone(), node_id.clone()))
                    .or_default()
                    .push(pid);
            }
        }
    }

    let mut tracked: Vec<TrackedProcess> = Vec::new();
    for workflow in &config.workflows {
        if let Some(filter) = workflow_id.as_deref() {
            if workflow.id != filter {
                continue;
            }
        }
        for node in &workflow.nodes {
            if node.kind != NodeKind::Application || node.app.path.trim().is_empty() {
                continue;
            }
            let target = platform::proc::resolve_path(&node.app.path)
                .unwrap_or_else(|| node.app.path.clone());
            let browser = is_browser_target(&target);
            let expected_name = match node
                .app
                .process_name
                .as_deref()
                .map(str::trim)
                .filter(|name| !name.is_empty())
            {
                Some(name) => process_stem(name),
                None => std::path::Path::new(&target)
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .map(process_stem)
                    .unwrap_or_default(),
            };

            let mut pids = recorded
                .get(&(workflow.id.clone(), node.id.clone()))
                .cloned()
                .unwrap_or_default();
            pids.sort_unstable();
            pids.dedup();

            if browser {
                if let Some(pid) = platform::proc::find_running_app(&node.app, &node.name) {
                    pids.push(pid);
                }
            } else {
                for process in &processes {
                    if pids.contains(&process.pid) {
                        continue;
                    }
                    let executable_match = process.executable.as_deref().is_some_and(|exe| {
                        exe.eq_ignore_ascii_case(&target)
                            || exe.eq_ignore_ascii_case(&node.app.path)
                    });
                    let name_match =
                        !expected_name.is_empty() && process_stem(&process.name) == expected_name;
                    if executable_match || name_match {
                        pids.push(process.pid);
                    }
                }
            }

            pids.sort_unstable();
            pids.dedup();
            for pid in pids {
                let Some(process) = processes.iter().find(|process| process.pid == pid) else {
                    continue;
                };
                tracked.push(TrackedProcess {
                    workflow_id: workflow.id.clone(),
                    workflow_name: workflow.name.clone(),
                    node_id: node.id.clone(),
                    node_name: node.name.clone(),
                    pid,
                    name: process.name.clone(),
                    executable: process.executable.clone(),
                    memory_bytes: process.memory_bytes,
                    cpu_usage: process.cpu_usage,
                    running_secs: process.running_secs,
                    managed: managed.contains_key(&pid),
                    main_window_title: process.main_window_title.clone(),
                });
            }
        }
    }

    tracked.sort_by(|left, right| {
        left.workflow_name
            .to_lowercase()
            .cmp(&right.workflow_name.to_lowercase())
            .then_with(|| {
                left.node_name
                    .to_lowercase()
                    .cmp(&right.node_name.to_lowercase())
            })
            .then_with(|| left.pid.cmp(&right.pid))
    });
    tracked
}

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
