//! 工作流 CRUD 与执行命令。

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, State};

use crate::config;
use crate::engine::{self, RunRequest};
use crate::model::{
    now_iso, AppLaunchSpec, GroupMode, LogLevel, NodeKind, RunTrigger, Workflow, WorkflowNode,
    WorkflowRun, WorkflowValidation,
};
use crate::state::AppState;

/// 补齐缺失字段，保证前端数据无论怎样都不会破坏图结构。
fn normalize(mut workflow: Workflow) -> Workflow {
    if workflow.id.trim().is_empty() {
        workflow.id = uuid::Uuid::new_v4().to_string();
    }
    if workflow.name.trim().is_empty() {
        workflow.name = "未命名工作流".into();
    }
    if workflow.created_at.is_empty() {
        workflow.created_at = now_iso();
    }
    workflow.name = workflow.name.trim().to_string();

    let mut seen: HashSet<String> = HashSet::new();
    for node in workflow.nodes.iter_mut() {
        if node.id.trim().is_empty() || !seen.insert(node.id.clone()) {
            node.id = uuid::Uuid::new_v4().to_string();
            seen.insert(node.id.clone());
        }
        if node.name.trim().is_empty() {
            node.name = default_node_name(node);
        }
        node.position.x = node.position.x.clamp(-5000.0, 20000.0);
        node.position.y = node.position.y.clamp(-5000.0, 20000.0);
        if node.kind == NodeKind::Delay {
            node.delay_ms = node.delay_ms.min(24 * 60 * 60 * 1000);
        }
    }

    let node_ids: HashSet<String> = workflow.nodes.iter().map(|n| n.id.clone()).collect();
    let mut edge_ids: HashSet<String> = HashSet::new();
    workflow.edges.retain_mut(|edge| {
        if !node_ids.contains(&edge.source)
            || !node_ids.contains(&edge.target)
            || edge.source == edge.target
        {
            return false;
        }
        if edge.id.trim().is_empty() || !edge_ids.insert(edge.id.clone()) {
            edge.id = uuid::Uuid::new_v4().to_string();
            edge_ids.insert(edge.id.clone());
        }
        true
    });

    let group_ids: HashSet<String> = workflow
        .nodes
        .iter()
        .filter(|node| node.kind == NodeKind::Group)
        .map(|node| node.id.clone())
        .collect();
    for node in workflow.nodes.iter_mut() {
        // 分组节点不参与执行，不能套娃到自身；子节点的 parent 必须指向存在的分组。
        let invalid_parent = node
            .parent_id
            .as_ref()
            .is_some_and(|parent| !group_ids.contains(parent) || parent == &node.id);
        if invalid_parent {
            node.parent_id = None;
        }
        if node.kind == NodeKind::Group {
            node.enabled = true;
            node.app = AppLaunchSpec::default();
            // 旧配置没有 group_mode：由 group_parallel 推导（并行/顺序），并保持兼容字段同步。
            let mode = node.group_mode.unwrap_or(if node.group_parallel {
                GroupMode::Parallel
            } else {
                GroupMode::Sequence
            });
            node.group_mode = Some(mode);
            node.group_parallel = mode == GroupMode::Parallel;
            node.group_delay_ms = node.group_delay_ms.min(600_000);
            node.group_parallel_limit = node.group_parallel_limit.min(1_000);
            node.group_parallel_interval_ms =
                node.group_parallel_interval_ms.min(600_000);
        }
    }

    workflow.touch();
    workflow
}

fn default_node_name(node: &WorkflowNode) -> String {
    match node.kind {
        NodeKind::Application => {
            let stem = std::path::Path::new(&node.app.path)
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            if stem.is_empty() {
                "新应用".into()
            } else {
                stem
            }
        }
        NodeKind::Delay => format!("等待 {} ms", node.delay_ms),
        NodeKind::Wait => {
            if node.wait.process_name.is_empty() {
                "等待进程".into()
            } else {
                format!("等待 {}", node.wait.process_name)
            }
        }
        NodeKind::Script => "执行命令".into(),
        NodeKind::Group => "新分组".into(),
    }
}

#[tauri::command]
pub fn list_workflows(state: State<'_, Arc<AppState>>) -> Vec<Workflow> {
    let mut workflows = state.config().workflows;
    workflows.sort_by_key(|workflow| workflow.name.to_lowercase());
    workflows
}

#[tauri::command]
pub fn save_workflow(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    workflow: Workflow,
) -> Result<Workflow, String> {
    let state = state.inner().clone();
    let workflow = normalize(workflow);

    // 每个工作流独立存成 `workflows/<id>.json`，config.json 只保留设置类数据。
    config::save_workflow_file(&workflow).map_err(|err| format!("保存工作流文件失败：{err}"))?;

    state
        .mutate_config(|config| {
            if let Some(existing) = config.workflow_mut(&workflow.id) {
                *existing = workflow.clone();
            } else {
                config.workflows.push(workflow.clone());
            }
        })
        .map_err(|err| format!("保存工作流失败：{err}"))?;

    state.log(
        &app,
        LogLevel::Info,
        "workflow",
        format!("已保存工作流「{}」", workflow.name),
        None,
    );
    Ok(workflow)
}

#[tauri::command]
pub fn delete_workflow(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    id: String,
) -> Result<(), String> {
    let state = state.inner().clone();
    if let Err(err) = config::delete_workflow_file(&id) {
        return Err(format!("删除工作流文件失败：{err}"));
    }
    let removed = state
        .mutate_config(|config| {
            let index = config.workflows.iter().position(|w| w.id == id)?;
            Some(config.workflows.remove(index).name)
        })
        .map_err(|err| format!("删除工作流失败：{err}"))?;

    match removed {
        Some(name) => {
            state.log(
                &app,
                LogLevel::Info,
                "workflow",
                format!("已删除工作流「{name}」"),
                Some(id),
            );
            Ok(())
        }
        None => Err("工作流不存在".into()),
    }
}

#[tauri::command]
pub fn duplicate_workflow(
    state: State<'_, Arc<AppState>>,
    id: String,
) -> Result<Workflow, String> {
    let state = state.inner().clone();
    let source = state
        .config()
        .workflow(&id)
        .cloned()
        .ok_or_else(|| "工作流不存在".to_string())?;

    let mut copy = source.clone();
    copy.id = uuid::Uuid::new_v4().to_string();
    copy.name = format!("{} 副本", source.name);
    copy.enabled = false;
    copy.created_at = now_iso();
    copy.touch();
    // 复制节点/边，保证 id 唯一
    let mut id_map = std::collections::HashMap::new();
    for node in copy.nodes.iter_mut() {
        let new_id = uuid::Uuid::new_v4().to_string();
        id_map.insert(node.id.clone(), new_id.clone());
        node.id = new_id;
    }
    // 分组归属也要跟着重映射到新 id。
    for node in copy.nodes.iter_mut() {
        if let Some(parent) = node.parent_id.clone() {
            node.parent_id = id_map.get(&parent).cloned();
        }
    }
    for edge in copy.edges.iter_mut() {
        edge.id = uuid::Uuid::new_v4().to_string();
        if let Some(new_source) = id_map.get(&edge.source) {
            edge.source = new_source.clone();
        }
        if let Some(new_target) = id_map.get(&edge.target) {
            edge.target = new_target.clone();
        }
    }
    let copy = normalize(copy);

    config::save_workflow_file(&copy).map_err(|err| format!("保存副本失败：{err}"))?;
    state
        .mutate_config(|config| config.workflows.push(copy.clone()))
        .map_err(|err| format!("复制工作流失败：{err}"))?;
    Ok(copy)
}

#[tauri::command]
pub fn validate_workflow(workflow: Workflow) -> WorkflowValidation {
    // 分组顺序/并行模式会忽略组内连线，校验应以实际执行图为准。
    engine::validate(&engine::effective_workflow(&workflow))
}

#[tauri::command]
pub fn run_workflow(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    workflow_id: String,
) -> Result<WorkflowRun, String> {
    let state = state.inner().clone();
    let workflow = state
        .config()
        .workflow(&workflow_id)
        .cloned()
        .ok_or_else(|| "工作流不存在".to_string())?;

    let validation = engine::validate(&engine::effective_workflow(&workflow));
    if !validation.ok {
        return Err(format!("工作流无法运行：{}", validation.errors.join("；")));
    }

    let run_id = engine::spawn_workflow_run(
        app.clone(),
        state.clone(),
        workflow.clone(),
        RunRequest {
            trigger: RunTrigger::Manual,
            delay: Duration::ZERO,
        },
    );
    state
        .run(&run_id)
        .ok_or_else(|| "创建运行记录失败".to_string())
}

/// 单点执行：用当前草稿中的某个节点发起一次只含该节点的调试运行。
#[tauri::command]
pub fn run_single_node(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    workflow: Workflow,
    node_id: String,
) -> Result<WorkflowRun, String> {
    let state = state.inner().clone();
    let run_id = engine::spawn_single_node_run(app, state.clone(), workflow, &node_id)?;
    state
        .run(&run_id)
        .ok_or_else(|| "创建运行记录失败".to_string())
}

/// 单点启动分组：把分组内所有可执行节点作为一次调试运行执行。
#[tauri::command]
pub fn run_single_group(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    workflow: Workflow,
    group_id: String,
) -> Result<WorkflowRun, String> {
    let state = state.inner().clone();
    let run_id = engine::spawn_group_run(app, state.clone(), workflow, &group_id)?;
    state
        .run(&run_id)
        .ok_or_else(|| "创建运行记录失败".to_string())
}

/// 运行所有启用中的工作流（供托盘菜单与登录启动复用）。
pub fn run_all_enabled_impl(
    app: &AppHandle,
    state: &Arc<AppState>,
    trigger: RunTrigger,
) -> Vec<WorkflowRun> {
    let config = state.config();
    let enabled: Vec<Workflow> = config.workflows.into_iter().filter(|w| w.enabled).collect();
    let mut runs = Vec::new();

    for workflow in enabled {
        let validation = engine::validate(&engine::effective_workflow(&workflow));
        if !validation.ok {
            state.log(
                app,
                LogLevel::Warn,
                "workflow",
                format!("跳过工作流「{}」：结构无效", workflow.name),
                Some(validation.errors.join("；")),
            );
            continue;
        }
        let run_id = engine::spawn_workflow_run(
            app.clone(),
            state.clone(),
            workflow,
            RunRequest {
                trigger,
                delay: Duration::ZERO,
            },
        );
        if let Some(run) = state.run(&run_id) {
            runs.push(run);
        }
    }
    runs
}

#[tauri::command]
pub fn run_all_enabled(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<WorkflowRun>, String> {
    let state = state.inner().clone();
    Ok(run_all_enabled_impl(&app, &state, RunTrigger::Manual))
}

#[tauri::command]
pub fn cancel_run(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    run_id: String,
) -> Result<(), String> {
    let state = state.inner().clone();
    if state.request_cancel(&run_id) {
        state.log(&app, LogLevel::Warn, "workflow", "已请求取消运行", Some(run_id));
        Ok(())
    } else {
        Err("运行不存在或已结束".into())
    }
}

#[tauri::command]
pub fn get_recent_runs(state: State<'_, Arc<AppState>>) -> Vec<WorkflowRun> {
    state.runs()
}

/// 编辑器里的「试运行」：不经过工作流，直接启动一个程序。
#[tauri::command]
pub fn test_launch(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    path: String,
    args: Option<Vec<String>>,
    working_dir: Option<String>,
) -> Result<u32, String> {
    let state = state.inner().clone();
    if path.trim().is_empty() {
        return Err("启动路径为空".into());
    }
    let spec = AppLaunchSpec {
        path: path.clone(),
        args: args.unwrap_or_default(),
        working_dir,
        skip_if_running: false,
        ..Default::default()
    };
    match crate::platform::launch_app(&spec) {
        Ok(pid) => {
            state.log(
                &app,
                LogLevel::Info,
                "launcher",
                format!("试启动：{path}"),
                Some(format!("pid={pid}")),
            );
            Ok(pid)
        }
        Err(err) => {
            state.log(
                &app,
                LogLevel::Error,
                "launcher",
                format!("试启动失败：{path}"),
                Some(err.to_string()),
            );
            Err(err.to_string())
        }
    }
}
