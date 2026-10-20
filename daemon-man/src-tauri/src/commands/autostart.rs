//! 自启动项扫描、开关与「接管」。

use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, State};

use crate::model::{
    now_iso, AppLaunchSpec, AutostartItem, AutostartOverride, LogLevel, NodeKind, ScriptSpec,
    Workflow, WorkflowNode,
};
use crate::platform;
use crate::state::AppState;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TakeOverResult {
    pub workflow: Workflow,
    pub items: Vec<AutostartItem>,
    /// 因不支持开关而跳过的项
    pub skipped: Vec<String>,
    /// 关闭失败的原因
    pub failures: Vec<String>,
}

#[tauri::command]
pub fn scan_autostart(
    state: State<'_, Arc<AppState>>,
    force: Option<bool>,
) -> Vec<AutostartItem> {
    let state = state.inner().clone();
    let config = state.config();
    let mut items = state.autostart_cached(force.unwrap_or(false));
    for item in items.iter_mut() {
        item.taken_over = config.overrides.iter().any(|o| o.id == item.id);
    }
    items
}

#[tauri::command]
pub fn set_autostart_enabled(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    item_id: String,
    enabled: bool,
) -> Result<AutostartItem, String> {
    let state = state.inner().clone();
    let item = platform::set_autostart_enabled(&item_id, enabled)
        .map_err(|err| format!("切换自启动项失败：{err}"))?;

    if enabled {
        state
            .mutate_config(|config| config.overrides.retain(|o| o.id != item_id))
            .map_err(|err| err.to_string())?;
    } else {
        let entry = AutostartOverride {
            id: item.id.clone(),
            name: item.name.clone(),
            source: item.source,
            disabled_at: now_iso(),
        };
        state
            .mutate_config(|config| {
                config.overrides.retain(|o| o.id != entry.id);
                config.overrides.push(entry);
            })
            .map_err(|err| err.to_string())?;
    }

    state.log(
        &app,
        LogLevel::Info,
        "autostart",
        format!(
            "自启动项「{}」已{}",
            item.name,
            if enabled { "启用" } else { "关闭" }
        ),
        Some(item.id.clone()),
    );
    Ok(item)
}

/// 把一个或多个自启动项关闭，并纳入工作流管线。
#[tauri::command]
pub fn take_over_autostart(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    item_ids: Vec<String>,
    workflow_id: Option<String>,
    workflow_name: Option<String>,
) -> Result<TakeOverResult, String> {
    let state = state.inner().clone();
    if item_ids.is_empty() {
        return Err("没有选择要接管的自启动项".into());
    }

    let all_items = state.autostart_cached(true);
    let config = state.config();

    let target_name = workflow_name
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| "接管自启动项".to_string());

    let mut workflow = if let Some(id) = workflow_id.as_ref() {
        config
            .workflow(id)
            .cloned()
            .ok_or_else(|| "目标工作流不存在".to_string())?
    } else if let Some(existing) = config.workflows.iter().find(|w| w.name == target_name) {
        existing.clone()
    } else {
        let mut created = Workflow::new(target_name);
        created.enabled = true;
        created
    };

    let mut skipped: Vec<String> = Vec::new();
    let mut failures: Vec<String> = Vec::new();
    let mut success_ids: Vec<String> = Vec::new();
    let mut new_overrides: Vec<AutostartOverride> = Vec::new();

    let mut next_y = workflow
        .nodes
        .iter()
        .map(|n| n.position.y)
        .fold(0.0_f64, f64::max)
        + 140.0;
    if workflow.nodes.is_empty() {
        next_y = 80.0;
    }

    let mut previous_node: Option<String> = workflow.nodes.last().map(|n| n.id.clone());

    for item_id in &item_ids {
        let Some(item) = all_items.iter().find(|i| &i.id == item_id) else {
            failures.push(format!("未找到自启动项：{item_id}"));
            continue;
        };

        if item.is_self {
            skipped.push(format!("{}（DaemonMan 自身）", item.name));
            continue;
        }
        if !item.toggle_supported {
            skipped.push(item.name.clone());
            continue;
        }

        // 关闭系统自启动；即使已经关闭也继续把它加入管线。
        if item.enabled {
            if let Err(err) = platform::set_autostart_enabled(&item.id, false) {
                failures.push(format!("关闭「{}」失败：{err}", item.name));
                continue;
            }
        }

        // 只有真实存在的可执行文件才用 Application 节点；
        // 计划任务里的裸命令（如 `FanControl`）或只带参数的 ComHandler 动作
        // 退化成 Script 节点，交给 `start` 由 shell 解析 PATH。
        let executable_exists = item
            .executable
            .as_deref()
            .map(|exe| std::path::Path::new(exe).exists())
            .unwrap_or(false);

        if !executable_exists && item.command.trim().is_empty() {
            skipped.push(format!("{}（无法解析启动命令）", item.name));
            continue;
        }

        let mut node = WorkflowNode {
            name: item.name.clone(),
            kind: NodeKind::Application,
            app: AppLaunchSpec {
                path: item.executable.clone().unwrap_or_default(),
                args: item
                    .args
                    .as_deref()
                    .map(split_args)
                    .unwrap_or_default(),
                skip_if_running: true,
                ..Default::default()
            },
            note: format!("接管自 {}｜{}", item.source_label, item.command),
            position: crate::model::NodePosition { x: 80.0, y: next_y },
            ..Default::default()
        };
        if !executable_exists {
            node.kind = NodeKind::Script;
            node.app = AppLaunchSpec::default();
            node.script = ScriptSpec {
                command: format!("start \"\" {}", item.command.trim()),
                ..Default::default()
            };
        }
        next_y += 140.0;

        if let Some(prev) = previous_node.take() {
            workflow.edges.push(crate::model::WorkflowEdge {
                id: uuid::Uuid::new_v4().to_string(),
                source: prev,
                target: node.id.clone(),
                condition: crate::model::EdgeCondition::Success,
            });
        }
        previous_node = Some(node.id.clone());
        workflow.nodes.push(node);

        new_overrides.push(AutostartOverride {
            id: item.id.clone(),
            name: item.name.clone(),
            source: item.source,
            disabled_at: now_iso(),
        });
        success_ids.push(item.id.clone());
    }

    if success_ids.is_empty() {
        return Err(if failures.is_empty() {
            "没有可接管的自启动项".to_string()
        } else {
            failures.join("；")
        });
    }

    workflow.touch();
    let workflow_for_return = workflow.clone();

    state
        .mutate_config(|config| {
            if let Some(existing) = config.workflow_mut(&workflow.id) {
                *existing = workflow.clone();
            } else {
                config.workflows.push(workflow.clone());
            }
            for ov in new_overrides.iter() {
                config.overrides.retain(|o| o.id != ov.id);
                config.overrides.push(ov.clone());
            }
        })
        .map_err(|err| format!("保存接管配置失败：{err}"))?;

    state.log(
        &app,
        LogLevel::Success,
        "autostart",
        format!(
            "已接管 {} 个自启动项到工作流「{}」",
            success_ids.len(),
            workflow_for_return.name
        ),
        if failures.is_empty() {
            None
        } else {
            Some(failures.join("；"))
        },
    );

    let config = state.config();
    let items = state
        .autostart_cached(true)
        .into_iter()
        .filter(|i| success_ids.contains(&i.id))
        .map(|mut i| {
            i.taken_over = config.overrides.iter().any(|o| o.id == i.id);
            i
        })
        .collect();

    Ok(TakeOverResult {
        workflow: workflow_for_return,
        items,
        skipped,
        failures,
    })
}

/// 取消接管：重新启用系统自启动项。
#[tauri::command]
pub fn release_autostart(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    item_id: String,
) -> Result<AutostartItem, String> {
    let state = state.inner().clone();
    let item = platform::set_autostart_enabled(&item_id, true)
        .map_err(|err| format!("恢复自启动项失败：{err}"))?;
    state
        .mutate_config(|config| config.overrides.retain(|o| o.id != item_id))
        .map_err(|err| err.to_string())?;
    state.autostart_cache_invalidate();
    state.log(
        &app,
        LogLevel::Info,
        "autostart",
        format!("已恢复自启动项「{}」", item.name),
        Some(item_id),
    );
    Ok(item)
}

/// 简单的命令行参数切分，支持双引号包裹。
fn split_args(raw: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    for ch in raw.chars() {
        match ch {
            '"' => in_quotes = !in_quotes,
            ' ' | '\t' if !in_quotes => {
                if !current.is_empty() {
                    args.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }
    if !current.is_empty() {
        args.push(current);
    }
    args
}
