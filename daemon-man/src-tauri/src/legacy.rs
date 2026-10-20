//! 旧版 ProgressManager（PowerShell 启动器）配置迁移。
//!
//! 旧配置是一棵树：
//! ```jsonc
//! { "launchApps": [
//!   { "type": "group", "name": "网络代理", "options": { "launchOrder": "ordered", "latency": 2000 },
//!     "launchApps": [ { "type": "program", "name": "Clash", "path": "…", "waiting": ["group | 基础工具"] } ] }
//! ]}
//! ```
//!
//! 迁移目标：一个 DaemonMan 工作流（保留跨分组的依赖，因此不拆成多个工作流）：
//! - 每个 program → Application 节点（`skip_if_running=true`，`closeMainWindow` 继承
//!   `afterStartup.sendCloseWindow`）；
//! - ordered 分组 → 子节点之间用 success 边串联，`latency` 用 Delay 节点表达；
//! - parallel 分组 → 子节点之间不建边（并行启动）；
//! - `waiting: ["group | X"]` / `["program | Y"]` → 从被依赖项的所有「出口节点」连边到
//!   本项（组为第一个/全部入口节点）。

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::model::{
    now_iso, AppConfig, AppLaunchSpec, EdgeCondition, NodeKind, NodePosition, Workflow,
    WorkflowEdge, WorkflowNode,
};

pub const WORKFLOW_NAME: &str = "ProgressManager 迁移";

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct LegacyConfig {
    launch_apps: Vec<LegacyItem>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct LegacyItem {
    #[serde(rename = "type")]
    kind: Option<String>,
    name: String,
    path: Option<String>,
    launch_apps: Vec<LegacyItem>,
    waiting: Vec<String>,
    options: LegacyOptions,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct LegacyOptions {
    launch_order: Option<String>,
    latency: Option<u64>,
    after_startup: Option<LegacyAfterStartup>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct LegacyAfterStartup {
    send_close_window: bool,
}

/// 迁移结果。
pub struct Converted {
    pub workflow: Workflow,
    pub warnings: Vec<String>,
}

#[derive(Default)]
struct Builder {
    workflow: Workflow,
    /// `type | name` → (入口节点, 出口节点)
    deps: HashMap<String, (Vec<String>, Vec<String>)>,
    /// 需要等所有依赖完成后才能开始的节点
    pending_waits: Vec<(Vec<String>, Vec<String>)>,
    warnings: Vec<String>,
    row: f64,
    column: usize,
}

impl Builder {
    fn new(workflow: Workflow) -> Self {
        Self {
            workflow,
            ..Default::default()
        }
    }

    fn position(&mut self) -> NodePosition {
        let position = NodePosition {
            x: 80.0 + self.column as f64 * 360.0,
            y: 80.0 + self.row * 150.0,
        };
        self.row += 1.0;
        position
    }

    fn push_node(&mut self, mut node: WorkflowNode) -> String {
        node.position = self.position();
        let id = node.id.clone();
        self.workflow.nodes.push(node);
        id
    }

    fn push_edge(&mut self, source: &str, target: &str) {
        self.workflow.edges.push(WorkflowEdge {
            id: uuid::Uuid::new_v4().to_string(),
            source: source.to_string(),
            target: target.to_string(),
            condition: EdgeCondition::Success,
        });
    }

    fn connect_all(&mut self, from: &[String], to: &[String]) {
        for source in from {
            for target in to {
                self.push_edge(source, target);
            }
        }
    }

    fn push_delay(&mut self, latency: u64) -> String {
        self.push_node(WorkflowNode {
            name: format!("等待 {latency} ms"),
            kind: NodeKind::Delay,
            delay_ms: latency,
            note: "旧版分组内的 launch latency".into(),
            ..Default::default()
        })
    }

    fn register_dep(&mut self, key: String, entry: Vec<String>, exited: Vec<String>) {
        let slot = self.deps.entry(key).or_default();
        for id in entry {
            if !slot.0.contains(&id) {
                slot.0.push(id);
            }
        }
        for id in exited {
            if !slot.1.contains(&id) {
                slot.1.push(id);
            }
        }
    }

    fn item_key(kind: &str, name: &str) -> String {
        format!("{} | {}", kind, name.trim())
    }

    /// 递归构建，返回 (入口节点, 出口节点)。
    ///
    /// 旧版语义：任何条目（包括 `type: "program"`）都可以再有 `launchApps` 子项；
    /// 程序先启动，然后按 `options.launchOrder` 启动子项。
    fn build_item(&mut self, item: &LegacyItem) -> (Vec<String>, Vec<String>) {
        let is_group = item.kind.as_deref() == Some("group");
        let kind = if is_group { "group" } else { "program" };
        let key = Self::item_key(kind, &item.name);

        // 1) 程序自身先成为一个 Application 节点。
        let own_entry: Option<String> = if is_group {
            None
        } else {
            if item.path.as_deref().unwrap_or("").trim().is_empty() {
                self.warnings.push(format!(
                    "程序「{}」缺少 path，已生成空路径节点（运行时需要手动补全）",
                    item.name
                ));
            }
            let node = WorkflowNode {
                name: item.name.clone(),
                kind: NodeKind::Application,
                app: AppLaunchSpec {
                    path: item.path.clone().unwrap_or_default(),
                    skip_if_running: true,
                    close_main_window: item
                        .options
                        .after_startup
                        .as_ref()
                        .map(|startup| startup.send_close_window)
                        .unwrap_or(false),
                    ..Default::default()
                },
                note: format!("旧版 ProgressManager 迁移：{}", item.name),
                ..Default::default()
            };
            Some(self.push_node(node))
        };

        // 2) 子项。
        let mut spans = Vec::new();
        for child in &item.launch_apps {
            spans.push(self.build_item(child));
        }

        let ordered = item
            .options
            .launch_order
            .as_deref()
            .map(|value| value != "parallel")
            .unwrap_or(true);
        let latency = item.options.latency.unwrap_or(0);

        let (entry, exited) = if spans.is_empty() {
            match own_entry.as_ref() {
                Some(id) => (vec![id.clone()], vec![id.clone()]),
                None => {
                    self.warnings
                        .push(format!("分组「{}」没有子项，已忽略", item.name));
                    (Vec::new(), Vec::new())
                }
            }
        } else if ordered {
            // 程序节点 → 第一个子项；组没有自身节点，入口就是第一个子项。
            if let Some(id) = own_entry.as_ref() {
                let first = spans[0].0.clone();
                if !first.is_empty() {
                    self.connect_all(std::slice::from_ref(id), &first);
                }
            }
            for index in 0..spans.len().saturating_sub(1) {
                let from = spans[index].1.clone();
                let to = spans[index + 1].0.clone();
                if from.is_empty() || to.is_empty() {
                    continue;
                }
                if latency > 0 {
                    let delay = self.push_delay(latency);
                    self.connect_all(&from, std::slice::from_ref(&delay));
                    self.connect_all(std::slice::from_ref(&delay), &to);
                } else {
                    self.connect_all(&from, &to);
                }
            }
            let entry = own_entry
                .clone()
                .map(|id| vec![id])
                .unwrap_or_else(|| spans[0].0.clone());
            let exited = spans[spans.len() - 1].1.clone();
            (entry, exited)
        } else {
            // 并行：程序节点（如有）放行所有子项；组则所有子项同时开始。
            if let Some(id) = own_entry.as_ref() {
                for (child_entry, _) in &spans {
                    self.connect_all(std::slice::from_ref(id), child_entry);
                }
            }
            let entry = own_entry
                .clone()
                .map(|id| vec![id])
                .unwrap_or_else(|| {
                    spans
                        .iter()
                        .flat_map(|(entry, _)| entry.clone())
                        .collect()
                });
            let exited: Vec<String> = spans
                .iter()
                .flat_map(|(_, exited)| exited.clone())
                .collect();
            (entry, exited)
        };

        self.register_dep(key, entry.clone(), exited.clone());
        if !item.waiting.is_empty() && !entry.is_empty() {
            self.pending_waits
                .push((entry.clone(), item.waiting.clone()));
        }
        (entry, exited)
    }

    fn finish(mut self) -> Converted {
        // 解析 waiting 引用（此时所有项都已注册）。
        let pending = std::mem::take(&mut self.pending_waits);
        for (targets, waiting) in pending {
            for raw in waiting {
                let raw = raw.trim();
                if raw.is_empty() {
                    continue;
                }
                let canonical = raw.replace('|', " | ");
                let (kind, name) = canonical
                    .split_once(" | ")
                    .map(|(kind, name)| (kind.trim().to_string(), name.trim().to_string()))
                    .unwrap_or_else(|| ("program".to_string(), canonical.trim().to_string()));
                let key = Self::item_key(&kind, &name);
                match self.deps.get(&key).cloned() {
                    Some((_, exits)) if !exits.is_empty() => {
                        self.connect_all(&exits, &targets);
                    }
                    Some(_) => {
                        self.warnings.push(format!("依赖「{raw}」没有出口节点，已跳过"));
                    }
                    None => {
                        self.warnings.push(format!("未找到依赖项「{raw}」，已跳过"));
                    }
                }
            }
        }

        // 依赖环检测（迁移出的图理论上无环；有环时强制失败而不是产出坏配置）。
        if let Err(error) = validate_graph(&self.workflow) {
            self.warnings.push(format!("迁移结果校验失败：{error}"));
        }

        Converted {
            workflow: self.workflow,
            warnings: self.warnings,
        }
    }
}

fn validate_graph(workflow: &Workflow) -> Result<()> {
    let validation = crate::engine::validate(workflow);
    if validation.ok {
        Ok(())
    } else {
        anyhow::bail!("{}", validation.errors.join("；"))
    }
}

fn convert_parsed(legacy: LegacyConfig, source: &str) -> Result<Converted> {
    let mut workflow = Workflow::new(WORKFLOW_NAME);
    workflow.enabled = true;
    workflow.description = format!(
        "由旧版 ProgressManager 配置迁移生成（来源：{source}）。保留分组内的顺序/并行、launch latency、waiting 依赖与启动后关闭窗口设置。"
    );

    let mut builder = Builder::new(workflow);
    for (column, item) in legacy.launch_apps.iter().enumerate() {
        builder.column = column;
        builder.row = 0.0;
        builder.build_item(item);
    }
    let converted = builder.finish();

    if converted.workflow.nodes.is_empty() {
        anyhow::bail!("旧配置里没有可迁移的启动项");
    }

    Ok(converted)
}

/// 读取并转换旧版配置文件。
pub fn convert_file(path: &Path) -> Result<Converted> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("读取旧版配置失败：{}", path.display()))?;
    // 旧版脚本用 PowerShell 写文件时带 UTF-8 BOM，serde_json 不接受 BOM。
    let text = text.trim_start_matches('\u{feff}');
    let legacy: LegacyConfig = serde_json::from_str(text)
        .with_context(|| format!("解析旧版配置失败：{}", path.display()))?;
    let mut converted = convert_parsed(legacy, &path.display().to_string())?;
    converted.workflow.touch();
    Ok(converted)
}

/// 将迁移出的工作流写进配置；同名工作流会被替换（幂等）。
pub fn import_into_config(config: &mut AppConfig, mut workflow: Workflow) -> bool {
    workflow.updated_at = now_iso();
    for existing in config.workflows.iter_mut() {
        if existing.name == workflow.name {
            *existing = workflow;
            return true;
        }
    }
    config.workflows.push(workflow);
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 与旧版 ProgressManager 的 config.json 结构一致的样例。
    const LEGACY: &str = r#"
    {
      "launchApps": [
        {
          "type": "group",
          "name": "Network Proxy Apps",
          "launchApps": [
            {
              "name": "Clash Verge",
              "path": "C:\\Program Files\\Clash Verge\\clash-verge.exe",
              "options": { "afterStartup": { "sendCloseWindow": true } }
            },
            {
              "type": "program",
              "name": "FlClash",
              "path": "C:\\Program Files\\FlClash\\FlClash.exe",
              "launchApps": [
                { "type": "program", "name": "v2rayN", "path": "D:\\v2rayN.exe" },
                { "name": "Clash Verge 2", "path": "D:\\clash2.exe" }
              ],
              "options": { "launchOrder": "parallel", "afterStartup": { "sendCloseWindow": true } }
            }
          ]
        },
        {
          "type": "group",
          "name": "Basic Background Utils",
          "launchApps": [
            { "name": "IPFS Desktop", "path": "C:\\IPFS Desktop.exe", "waiting": ["group | Network Proxy Apps"] },
            { "name": "Sandboxie-Plus", "path": "C:\\SandMan.exe" }
          ]
        },
        {
          "type": "group",
          "name": "IM Apps",
          "launchApps": [
            { "name": "Telegram", "path": "C:\\Telegram.exe" },
            { "name": "Feishu", "path": "C:\\Feishu.exe", "waiting": ["program | Sandboxie-Plus"] }
          ],
          "waiting": ["group | Network Proxy Apps"],
          "options": { "launchOrder": "ordered", "latency": 2000 }
        }
      ]
    }
    "#;

    #[test]
    fn converts_programs_groups_and_delays() {
        let legacy: LegacyConfig = serde_json::from_str(LEGACY).unwrap();
        let converted = convert_parsed(legacy, "test").unwrap();
        let workflow = converted.workflow;

        // 8 个程序节点 + 1 个 latency 延时节点（IM Apps 有 2 个子项 → 中间 1 个 delay）
        assert_eq!(workflow.nodes.len(), 9, "nodes: {:?}", workflow.nodes.iter().map(|n| &n.name).collect::<Vec<_>>());
        assert_eq!(
            workflow.nodes.iter().filter(|n| n.kind == NodeKind::Application).count(),
            8
        );
        assert_eq!(
            workflow.nodes.iter().filter(|n| n.kind == NodeKind::Delay).count(),
            1
        );
        assert!(workflow.enabled, "迁移后的工作流应默认随登录启动");

        // 关闭窗口选项被保留
        let clash = workflow
            .nodes
            .iter()
            .find(|n| n.name == "Clash Verge")
            .unwrap();
        assert!(clash.app.close_main_window);
        assert!(clash.app.skip_if_running);

        // parallel 分组的子项：FlClash 与其两个并行子项之间有连线，但子项之间没有。
        let flclash = workflow.nodes.iter().find(|n| n.name == "FlClash").unwrap();
        let v2ray = workflow.nodes.iter().find(|n| n.name == "v2rayN").unwrap();
        let clash2 = workflow.nodes.iter().find(|n| n.name == "Clash Verge 2").unwrap();
        assert_eq!(
            workflow.edges.iter().filter(|e| e.source == flclash.id).count(),
            2,
            "FlClash 应放行两个并行子项"
        );
        assert!(!workflow
            .edges
            .iter()
            .any(|e| (e.source == v2ray.id && e.target == clash2.id)
                || (e.source == clash2.id && e.target == v2ray.id)));

        // ordered 分组的子项之间串联（Telegram → delay → Feishu）。
        let telegram = workflow.nodes.iter().find(|n| n.name == "Telegram").unwrap();
        let feishu = workflow.nodes.iter().find(|n| n.name == "Feishu").unwrap();
        assert!(workflow
            .edges
            .iter()
            .any(|e| e.source == telegram.id));
        assert!(workflow
            .edges
            .iter()
            .any(|e| e.target == feishu.id));
        assert!(!workflow
            .edges
            .iter()
            .any(|e| e.source == feishu.id && e.target == telegram.id));
    }

    #[test]
    fn waiting_edges_are_resolved() {
        let legacy: LegacyConfig = serde_json::from_str(LEGACY).unwrap();
        let converted = convert_parsed(legacy, "test").unwrap();
        let workflow = converted.workflow;
        let ipfs = workflow.nodes.iter().find(|n| n.name == "IPFS Desktop").unwrap();
        assert!(
            workflow.edges.iter().any(|e| e.target == ipfs.id),
            "IPFS Desktop 应等待 Network Proxy Apps 分组"
        );
    }

    #[test]
    fn unknown_dependency_is_reported_as_warning() {
        let raw = r#"{ "launchApps": [ { "name": "A", "path": "C:\\a.exe", "waiting": ["group | 不存在"] } ] }"#;
        let legacy: LegacyConfig = serde_json::from_str(raw).unwrap();
        let converted = convert_parsed(legacy, "test").unwrap();
        assert!(converted.warnings.iter().any(|w| w.contains("不存在")), "{:?}", converted.warnings);
    }

    #[test]
    fn utf8_bom_is_tolerated() {
        let text = format!("\u{feff}{LEGACY}");
        let trimmed = text.trim_start_matches('\u{feff}');
        let legacy: LegacyConfig = serde_json::from_str(trimmed).unwrap();
        assert_eq!(legacy.launch_apps.len(), 3);
    }

    #[test]
    fn import_replaces_same_named_workflow() {
        let mut config = AppConfig::new();
        let mut workflow = Workflow::new(WORKFLOW_NAME);
        workflow.nodes.push(WorkflowNode::default());
        assert!(!import_into_config(&mut config, workflow.clone()));
        assert_eq!(config.workflows.len(), 1);
        assert!(import_into_config(&mut config, workflow));
        assert_eq!(config.workflows.len(), 1);
    }
}
