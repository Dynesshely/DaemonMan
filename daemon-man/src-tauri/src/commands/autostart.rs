//! 自启动项扫描、开关与「接管」。

use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, State};

use crate::model::{
    now_iso, AppLaunchSpec, AutostartItem, AutostartOverride, AutostartSource, LogLevel, NodeKind,
    ScriptSpec, Workflow, WorkflowNode,
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
            mode: "native".into(),
            scope: item.scope.clone(),
            command: item.command.clone(),
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
            mode: "native".into(),
            scope: item.scope.clone(),
            command: item.command.clone(),
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

/// 取消接管 / 从备份恢复：重新启用系统自启动项。
#[tauri::command]
pub fn release_autostart(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    item_id: String,
) -> Result<AutostartItem, String> {
    let state = state.inner().clone();
    let item = platform::restore_autostart(&item_id)
        .map_err(|err| format!("恢复自启动项失败：{err:#}"))?;
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

/// 右键「禁用」：支持的项用系统开关；不支持的项备份到配置目录后从原位置移除。
#[tauri::command]
pub fn disable_autostart_item(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    item_id: String,
) -> Result<AutostartItem, String> {
    let state = state.inner().clone();
    let item = state
        .autostart_cached(true)
        .into_iter()
        .find(|candidate| candidate.id == item_id)
        .ok_or_else(|| "未找到自启动项".to_string())?;

    if item.is_self {
        return Err("不能禁用 DaemonMan 自身".into());
    }

    platform::disable_autostart(&item).map_err(|err| format!("禁用失败：{err:#}"))?;
    let mode = if item.toggle_supported { "native" } else { "removed" };

    let entry = AutostartOverride {
        id: item.id.clone(),
        name: item.name.clone(),
        source: item.source,
        disabled_at: now_iso(),
        mode: mode.to_string(),
        scope: item.scope.clone(),
        command: item.command.clone(),
    };
    state
        .mutate_config(|config| {
            config.overrides.retain(|o| o.id != entry.id);
            config.overrides.push(entry);
        })
        .map_err(|err| err.to_string())?;
    state.autostart_cache_invalidate();

    state.log(
        &app,
        LogLevel::Info,
        "autostart",
        format!(
            "自启动项「{}」已禁用（{}）",
            item.name,
            if mode == "native" {
                "系统开关"
            } else {
                "已备份并移除"
            }
        ),
        Some(item.id.clone()),
    );

    let mut updated = state
        .autostart_cached(true)
        .into_iter()
        .find(|candidate| candidate.id == item_id)
        .unwrap_or(item);
    updated.enabled = false;
    updated.taken_over = true;
    Ok(updated)
}

/// 右键「恢复」：与取消接管等价，同时支持从备份还原被移除的项。
#[tauri::command]
pub fn restore_autostart_item(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    item_id: String,
) -> Result<AutostartItem, String> {
    let state = state.inner().clone();
    let item = platform::restore_autostart(&item_id)
        .map_err(|err| format!("恢复失败：{err:#}"))?;
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

/// 右键「删除」：先把数据备份到配置目录，再从原位置永久删除。
#[tauri::command]
pub fn delete_autostart_item(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    item_id: String,
) -> Result<(), String> {
    let state = state.inner().clone();
    let item = state
        .autostart_cached(true)
        .into_iter()
        .find(|candidate| candidate.id == item_id)
        .ok_or_else(|| "未找到自启动项".to_string())?;

    if item.is_self {
        return Err("不能删除 DaemonMan 自身".into());
    }
    if !item.can_delete {
        return Err("该类型的自启动项不支持删除（系统服务请使用禁用）".into());
    }

    platform::delete_autostart(&item).map_err(|err| format!("删除失败：{err:#}"))?;
    state
        .mutate_config(|config| config.overrides.retain(|o| o.id != item_id))
        .map_err(|err| err.to_string())?;
    state.autostart_cache_invalidate();
    state.log(
        &app,
        LogLevel::Warn,
        "autostart",
        format!(
            "已删除自启动项「{}」（原数据已备份到配置目录的 autostart-backup/）",
            item.name
        ),
        Some(item_id),
    );
    Ok(())
}

/// 可在内置编辑器中打开的脚本类扩展名。
const EDITABLE_EXTENSIONS: &[&str] = &[
    "bat", "cmd", "ps1", "vbs", "vbe", "js", "jse", "wsf", "reg", "txt", "ini", "sh", "py",
];
const MAX_SCRIPT_BYTES: u64 = 512 * 1024;

/// 启动脚本内容（用于右键「编辑启动内容」）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptContent {
    pub path: String,
    pub content: String,
    pub size: u64,
    /// `utf-8` / `utf-8-bom` / `gb18030`
    pub encoding: String,
}

fn is_editable_script(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_lowercase())
        .is_some_and(|extension| EDITABLE_EXTENSIONS.contains(&extension.as_str()))
}

/// 解析可编辑的脚本路径：启动文件夹的 locator 文件优先，其次是可执行文件。
fn editable_script_path(item: &AutostartItem) -> Option<std::path::PathBuf> {
    let mut candidates: Vec<&str> = vec![item.locator.as_str()];
    if let Some(executable) = item.executable.as_deref() {
        if !candidates.contains(&executable) {
            candidates.push(executable);
        }
    }
    candidates.into_iter().find_map(|candidate| {
        if candidate.trim().is_empty() {
            return None;
        }
        let path = std::path::Path::new(candidate);
        if path.is_file() && is_editable_script(path) {
            Some(path.to_path_buf())
        } else {
            None
        }
    })
}

/// 识别脚本编码：BOM → UTF-8；UTF-16 BOM → UTF-16；纯 ASCII 的批处理类文件按 GB18030（中文 Windows 的 cmd 默认代码页）；
/// 其余能按 UTF-8 解码就是 UTF-8，否则按 GB18030。
fn decode_script(path: &std::path::Path, bytes: &[u8]) -> (String, &'static str) {
    if let Some(without_bom) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        if let Ok(text) = std::str::from_utf8(without_bom) {
            return (text.to_string(), "utf-8-bom");
        }
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        let units: Vec<u16> = rest
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .collect();
        return (String::from_utf16_lossy(&units), "utf-16le");
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        let units: Vec<u16> = rest
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_be_bytes(*pair))
            .collect();
        return (String::from_utf16_lossy(&units), "utf-16be");
    }

    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_lowercase())
        .unwrap_or_default();
    let legacy_script = matches!(
        extension.as_str(),
        "bat" | "cmd" | "vbs" | "vbe" | "reg" | "ini"
    );

    match std::str::from_utf8(bytes) {
        Ok(text) if legacy_script && bytes.is_ascii() => (text.to_string(), "gb18030"),
        Ok(text) => (text.to_string(), "utf-8"),
        Err(_) => {
            let (decoded, _, _) = encoding_rs::GB18030.decode(bytes);
            (decoded.to_string(), "gb18030")
        }
    }
}

fn encode_script(content: &str, encoding: &str) -> Vec<u8> {
    match encoding {
        "utf-8-bom" => {
            let mut bytes = vec![0xEF, 0xBB, 0xBF];
            bytes.extend_from_slice(content.as_bytes());
            bytes
        }
        "utf-16le" => {
            let mut bytes = vec![0xFF, 0xFE];
            for unit in content.encode_utf16() {
                bytes.extend_from_slice(&unit.to_le_bytes());
            }
            bytes
        }
        "utf-16be" => {
            let mut bytes = vec![0xFE, 0xFF];
            for unit in content.encode_utf16() {
                bytes.extend_from_slice(&unit.to_be_bytes());
            }
            bytes
        }
        "gb18030" => {
            let (bytes, _, _) = encoding_rs::GB18030.encode(content);
            bytes.to_vec()
        }
        _ => content.as_bytes().to_vec(),
    }
}

fn find_autostart_item(
    state: &AppState,
    item_id: &str,
) -> Result<AutostartItem, String> {
    state
        .autostart_cached(true)
        .into_iter()
        .find(|item| item.id == item_id)
        .ok_or_else(|| "未找到自启动项（可能已被移除）".to_string())
}

/// 右键「在资源管理器中显示」：定位可执行文件或启动文件夹里的脚本文件。
#[tauri::command]
pub fn reveal_autostart_item(
    state: State<'_, Arc<AppState>>,
    item_id: String,
) -> Result<String, String> {
    let state = state.inner().clone();
    let item = find_autostart_item(&state, &item_id)?;

    let mut candidates: Vec<String> = Vec::new();
    if let Some(executable) = item.executable.clone() {
        candidates.push(executable);
    }
    if item.source == AutostartSource::StartupFolder {
        candidates.push(item.locator.clone());
    }

    let target = candidates
        .into_iter()
        .find(|candidate| !candidate.trim().is_empty() && std::path::Path::new(candidate).exists());

    match target {
        Some(path) => {
            std::process::Command::new("explorer")
                .arg(format!("/select,{path}"))
                .spawn()
                .map_err(|error| format!("打开资源管理器失败：{error}"))?;
            Ok(path)
        }
        None => {
            // 文件不存在时，退一步打开它的父目录（如果父目录存在）。
            if let Some(parent) = item
                .executable
                .as_deref()
                .and_then(|executable| std::path::Path::new(executable).parent())
                .filter(|parent| parent.exists())
            {
                std::process::Command::new("explorer")
                    .arg(parent)
                    .spawn()
                    .map_err(|error| format!("打开资源管理器失败：{error}"))?;
                return Ok(parent.display().to_string());
            }
            Err("该启动项没有可定位的文件".into())
        }
    }
}

/// 右键「编辑启动内容」：读取脚本文件（自动识别 UTF-8 / GBK）。
#[tauri::command]
pub fn read_autostart_script(
    state: State<'_, Arc<AppState>>,
    item_id: String,
) -> Result<ScriptContent, String> {
    let state = state.inner().clone();
    let item = find_autostart_item(&state, &item_id)?;
    let path = editable_script_path(&item).ok_or_else(|| {
        "该启动项不是可编辑的脚本文件（支持 .bat/.cmd/.ps1/.vbs/.js/.reg/.txt 等）".to_string()
    })?;

    let bytes = std::fs::read(&path).map_err(|error| format!("读取脚本失败：{error}"))?;
    if bytes.len() as u64 > MAX_SCRIPT_BYTES {
        return Err(format!(
            "脚本超过 {} KB，无法在编辑器打开",
            MAX_SCRIPT_BYTES / 1024
        ));
    }
    let (content, encoding) = decode_script(&path, &bytes);
    Ok(ScriptContent {
        path: path.display().to_string(),
        content,
        size: bytes.len() as u64,
        encoding: encoding.to_string(),
    })
}

/// 右键「编辑启动内容」：保存脚本（写前自动备份为 `<文件名>.daemonman.bak`，保持原编码）。
#[tauri::command]
pub fn write_autostart_script(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    item_id: String,
    content: String,
    encoding: Option<String>,
) -> Result<ScriptContent, String> {
    let state = state.inner().clone();
    let item = find_autostart_item(&state, &item_id)?;
    let path = editable_script_path(&item)
        .ok_or_else(|| "该启动项不是可编辑的脚本文件".to_string())?;

    if content.len() as u64 > MAX_SCRIPT_BYTES {
        return Err(format!(
            "内容超过 {} KB，无法保存",
            MAX_SCRIPT_BYTES / 1024
        ));
    }

    let original_bytes =
        std::fs::read(&path).map_err(|error| format!("读取原脚本失败：{error}"))?;
    let detected = decode_script(&path, &original_bytes).1;
    // 前端会带回来读取时的编码；参数缺省/非法时回退到自动检测，避免把 GBK 批处理写成 UTF-8。
    let encoding = match encoding.as_deref() {
        Some(value)
            if matches!(
                value,
                "utf-8" | "utf-8-bom" | "gb18030" | "utf-16le" | "utf-16be"
            ) =>
        {
            value
        }
        _ => detected,
    };

    // 备份到 DaemonMan 的配置目录，避免在启动文件夹里留下 .bak 文件。
    crate::config::ensure_dirs().map_err(|error| format!("创建备份目录失败：{error}"))?;
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("script");
    let backup = crate::config::backup_dir().join(format!("script-{stamp}-{file_name}.bak"));
    std::fs::copy(&path, &backup)
        .map_err(|error| format!("备份原脚本失败（已中止保存）：{error}"))?;

    let bytes = encode_script(&content, encoding);
    std::fs::write(&path, &bytes).map_err(|error| format!("写入脚本失败：{error}"))?;

    state.log(
        &app,
        LogLevel::Info,
        "autostart",
        format!("已编辑启动脚本「{}」", path.display()),
        Some(format!("原文件备份：{}", backup.display())),
    );

    Ok(ScriptContent {
        path: path.display().to_string(),
        content,
        size: bytes.len() as u64,
        encoding: encoding.to_string(),
    })
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
