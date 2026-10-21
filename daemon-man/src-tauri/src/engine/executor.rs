//! 单次工作流运行的调度器。
//!
//! 调度模型是「静态依赖 + 动态放行」：
//! - 每个节点记录自己的入边（上游节点 + 连线条件）；
//! - 上游有终态后，按 success/failure/always 判断这条边是否放行；
//! - 全部入边都放行后节点才可执行；没有入边的节点在运行开始时并行启动；
//! - 只要有入边被确定永远无法满足（例如上游成功但要求 failure 边），节点立即跳过。
//!
//! 两类节点完全不参与执行：`NodeKind::Group`（只用于展示，`build_plan` 与
//! `build_initial_run` 都会排除，涉及它的连线在依赖判定中被忽略）；`enabled == false`
//! 的节点会被跳过（对下游算成功）。
//!
//! 「已运行则跳过」是两级开关：`workflow.skip_existing_processes && node.app.skip_if_running`
//! （见 [`skip_existing_enabled`]）。生效且进程确实在运行时，节点状态是
//! [`NodeStatus::SkippedExisting`]，对下游等同成功；工作流级开关关闭时一定会真正启动。
//!
//! 并发由 `settings.max_concurrency` 控制（`tokio::sync::Semaphore`）；每个节点整体包裹在
//! `tokio::time::timeout` 中，但超时值按节点类型计算（见 [`effective_timeout`]），避免
//! 「延迟 30s+」「等待进程 60s」这类本身有明确时长的节点被全局默认值误判失败；
//! 有效超时为 0 时不包 timeout（0 = 不限）。
//!
//! 取消标记是 `state.run_flag(run_id)` 返回的 `Arc<AtomicBool>`，调度器与各节点执行体
//! 都会主动检查；已经启动的进程不会被强制结束。

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use std::time::Duration;

use tauri::{AppHandle, Emitter};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use crate::engine::{graph, launcher};
use crate::model::{
    now_iso, EdgeCondition, GroupMode, LogLevel, NodeKind, NodeRunState, NodeStatus, RunStatus,
    RunTrigger, ScriptSpec, Workflow, WorkflowEdge, WorkflowNode, WorkflowRun,
};
use crate::platform;
use crate::state::AppState;

/// 取消标记的轮询间隔。
const CANCEL_POLL_INTERVAL: Duration = Duration::from_millis(50);
/// 调度主循环在等待任务完成时的轮询间隔（用于响应取消）。
const SCHEDULER_POLL_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug, Clone)]
pub struct RunRequest {
    pub trigger: RunTrigger,
    /// 延迟多久后开始（用于登录启动时的缓冲）。
    pub delay: Duration,
}

impl Default for RunRequest {
    fn default() -> Self {
        Self {
            trigger: RunTrigger::Manual,
            delay: Duration::ZERO,
        }
    }
}

// ---------------------------------------------------------------------------
// 纯逻辑：依赖条件判定
// ---------------------------------------------------------------------------

/// 上游节点对下游而言的「有效终态」。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpstreamOutcome {
    Success,
    Failed,
}

/// 节点执行的内部结果，比 [`NodeStatus`] 多一层调度语义：
/// - `SkippedExisting` 表示进程已在运行而跳过启动，对下游等同于成功；
/// - `Disabled` 表示节点被用户禁用，对下游等同于成功（不阻断流程）；
/// - `Skipped` 表示因为依赖条件不满足而跳过，对下游等同于失败（继续沿失败边传播）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeOutcome {
    Success,
    SkippedExisting,
    Failed,
    Skipped,
    Disabled,
    Cancelled,
}

impl NodeOutcome {
    pub fn upstream(self) -> UpstreamOutcome {
        match self {
            NodeOutcome::Success | NodeOutcome::SkippedExisting | NodeOutcome::Disabled => {
                UpstreamOutcome::Success
            }
            NodeOutcome::Failed | NodeOutcome::Skipped | NodeOutcome::Cancelled => {
                UpstreamOutcome::Failed
            }
        }
    }
}

/// 一条入边：上游节点 id、上游展示名、连线条件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncomingEdge {
    pub source: String,
    pub source_label: String,
    pub condition: EdgeCondition,
}

/// 节点的可执行状态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Readiness {
    /// 全部入边都满足，可以执行。
    Ready,
    /// 还有上游节点没有终态，继续等待。
    Waiting,
    /// 可以确定某条入边永远无法满足，节点应跳过（消息说明原因）。
    Blocked(String),
}

/// 是否启用「已运行则跳过」：工作流级开关与节点级开关同时为真。
///
/// 工作流级开关为 false 时，无论节点如何配置都必须真正执行启动。
pub fn skip_existing_enabled(workflow_skip: bool, node_skip: bool) -> bool {
    workflow_skip && node_skip
}

/// 单条入边是否放行。
pub fn edge_allows(condition: EdgeCondition, upstream: UpstreamOutcome) -> bool {
    match condition {
        EdgeCondition::Success => upstream == UpstreamOutcome::Success,
        EdgeCondition::Failure => upstream == UpstreamOutcome::Failed,
        EdgeCondition::Always => true,
    }
}

/// 根据上游终态判断节点是否可执行。
///
/// 只要有入边已经确定不满足就返回 [`Readiness::Blocked`]（早跳过，让下游尽快继续），
/// 否则还有未结束的上游时返回 [`Readiness::Waiting`]。
pub fn evaluate_readiness(
    incoming: &[IncomingEdge],
    outcomes: &HashMap<String, NodeOutcome>,
) -> Readiness {
    let mut blocked: Vec<String> = Vec::new();
    let mut waiting = false;

    for edge in incoming {
        match outcomes.get(&edge.source) {
            None => waiting = true,
            Some(outcome) => {
                if !edge_allows(edge.condition, outcome.upstream()) {
                    blocked.push(format!(
                        "上游节点「{}」{}，不满足「{}」连线条件",
                        edge.source_label,
                        describe_outcome(*outcome),
                        describe_condition(edge.condition)
                    ));
                }
            }
        }
    }

    if !blocked.is_empty() {
        Readiness::Blocked(blocked.join("；"))
    } else if waiting {
        Readiness::Waiting
    } else {
        Readiness::Ready
    }
}

fn describe_outcome(outcome: NodeOutcome) -> &'static str {
    match outcome {
        NodeOutcome::Success => "执行成功",
        NodeOutcome::SkippedExisting => "已在运行，跳过启动",
        NodeOutcome::Failed => "执行失败",
        NodeOutcome::Skipped => "被跳过",
        NodeOutcome::Disabled => "已禁用",
        NodeOutcome::Cancelled => "被取消",
    }
}

fn describe_condition(condition: EdgeCondition) -> &'static str {
    match condition {
        EdgeCondition::Success => "仅成功时执行",
        EdgeCondition::Failure => "仅失败时执行",
        EdgeCondition::Always => "总是执行",
    }
}

// ---------------------------------------------------------------------------
// 取消辅助
// ---------------------------------------------------------------------------

pub(crate) fn is_cancelled(flag: &Arc<AtomicBool>) -> bool {
    flag.load(Ordering::SeqCst)
}

/// 分段睡眠并持续检查取消标记；返回 `false` 表示被取消。
pub(crate) async fn sleep_cancellable(duration: Duration, cancel: &Arc<AtomicBool>) -> bool {
    if is_cancelled(cancel) {
        return false;
    }
    let deadline = tokio::time::Instant::now() + duration;
    loop {
        let now = tokio::time::Instant::now();
        if now >= deadline {
            return true;
        }
        if is_cancelled(cancel) {
            return false;
        }
        let step = (deadline - now).min(CANCEL_POLL_INTERVAL);
        tokio::time::sleep(step).await;
    }
}

// ---------------------------------------------------------------------------
// 入口
// ---------------------------------------------------------------------------

/// 创建一个运行记录并异步执行，返回 run_id。
///
/// 函数本身立即返回；真正的执行在 `tauri::async_runtime::spawn` 的任务里完成，
/// 每次节点状态变化都会 `upsert_run` 并 emit `workflow:run`。
pub fn spawn_workflow_run(
    app: AppHandle,
    state: Arc<AppState>,
    workflow: Workflow,
    request: RunRequest,
) -> String {
    // 按分组模式生成「有效执行图」（纯副本，不写回持久化数据）。
    // 单点执行 / 分组执行的子工作流不含分组节点，这里是 no-op。
    let workflow = effective_workflow(&workflow);

    let run_id = uuid::Uuid::new_v4().to_string();
    let initial = build_initial_run(&workflow, &run_id, request.trigger);

    state.upsert_run(initial.clone());
    let cancel = state.register_run_flag(&run_id);
    let _ = app.emit("workflow:run", &initial);

    // 结构无效时不开任务，直接把这次运行标记为失败（正常入口 commands 已先校验过）。
    let validation = graph::validate(&workflow);
    if !validation.ok {
        let mut failed = initial;
        for node in failed.nodes.values_mut() {
            node.status = NodeStatus::Skipped;
            node.message = Some("工作流结构无效，未执行".to_string());
            node.finished_at = Some(now_iso());
        }
        failed.status = RunStatus::Failed;
        failed.message = Some(format!("工作流结构无效：{}", validation.errors.join("；")));
        failed.finished_at = Some(now_iso());

        state.upsert_run(failed.clone());
        let _ = app.emit("workflow:run", &failed);
        state.log(
            &app,
            LogLevel::Error,
            "workflow",
            format!("工作流「{}」结构无效，已拒绝执行", workflow.name),
            Some(validation.errors.join("；")),
        );
        state.release_run_flag(&run_id);
        return run_id;
    }

    let task_app = app.clone();
    let task_state = state.clone();
    let task_run_id = run_id.clone();
    tauri::async_runtime::spawn(async move {
        run_task(
            task_app,
            task_state,
            workflow,
            initial,
            request,
            task_run_id,
            cancel,
        )
        .await;
    });

    run_id
}

/// 只运行工作流里的某一个节点（编辑器「单点执行」调试）。
///
/// 与 [`spawn_workflow_run`] 的区别：
/// - **不校验整张图**（未保存的草稿也能单点执行），只要求节点存在且不是分组节点；
/// - 节点会被复制并强制 `enabled = true`（即使它在工作流里被禁用，也允许单独调试）；
/// - 构造只含该节点的最小工作流（`edges` 为空，`id` / `name` / `skip_existing_processes`
///   等沿用原工作流），所以 `run.workflow_id` 仍是原工作流 id、节点 id 保持不变，
///   前端继续用 `workflow:run` 事件按 nodeId 更新状态；
/// - 走与完整运行完全相同的执行管线（Manual 触发、并发/取消/超时语义一致、
///   完整 `workflow:run` 事件与 `log:entry` 日志，run 进入运行历史）。
///
/// 返回创建出的运行 id（与 [`spawn_workflow_run`] 对齐，调用方可用 `state.run(&run_id)`
/// 取回完整 [`WorkflowRun`]）；节点不存在或为分组节点时返回中文 `Err`。
pub fn spawn_single_node_run(
    app: AppHandle,
    state: Arc<AppState>,
    workflow: Workflow,
    node_id: &str,
) -> Result<String, String> {
    let node = resolve_single_node(&workflow, node_id)?;
    let mini = build_single_node_workflow(&workflow, node);

    Ok(spawn_workflow_run(
        app,
        state,
        mini,
        RunRequest {
            trigger: RunTrigger::Manual,
            delay: Duration::ZERO,
        },
    ))
}

/// 单点执行的目标节点：必须存在且不是分组节点。
fn resolve_single_node<'a>(
    workflow: &'a Workflow,
    node_id: &str,
) -> Result<&'a WorkflowNode, String> {
    let node = workflow
        .nodes
        .iter()
        .find(|node| node.id == node_id)
        .ok_or_else(|| format!("节点不存在：{node_id}"))?;
    if node.kind == NodeKind::Group {
        return Err("分组节点不支持单点执行".to_string());
    }
    Ok(node)
}

/// 构造「只含一个节点」的最小工作流：节点克隆后强制 `enabled = true`，
/// `edges` 清空，其余字段沿用原工作流。
fn build_single_node_workflow(workflow: &Workflow, node: &WorkflowNode) -> Workflow {
    let mut single = node.clone();
    single.enabled = true;
    Workflow {
        nodes: vec![single],
        edges: Vec::new(),
        ..workflow.clone()
    }
}

/// 只运行某个分组下的全部节点（右键分组「单点启动分组」）。
///
/// 语义：
/// - 校验 `group_id` 对应一个分组节点（不存在 / 不是分组 → 中文 `Err`）；
/// - 递归收集该分组的所有后代（沿 `parent_id` 链，包含嵌套分组里的节点），
///   排除分组节点本身；没有可执行节点时返回 `Err`；
/// - 构造子工作流：`id` / `name` / `enabled` / `skip_existing_processes` 等沿用原工作流，
///   `nodes` 是后代可执行节点（**强制 `enabled = true`**，便于调试），
///   `edges` 只保留两端都在集合内的边（内部依赖保留，跨组边丢弃）；
/// - 不校验整图，直接复用 [`spawn_workflow_run`]（Manual、delay 0、完整事件/日志/历史），
///   `run.workflow_id` 仍是原工作流 id、节点 id 不变。
///
/// 返回 run id（与 [`spawn_single_node_run`] 一致）。
pub fn spawn_group_run(
    app: AppHandle,
    state: Arc<AppState>,
    workflow: Workflow,
    group_id: &str,
) -> Result<String, String> {
    // 先按分组模式生成有效执行图，再从中抽取目标分组：
    // 这样分组启动遵循自身的顺序/并行语义，合成延迟节点也会一并执行。
    let effective = effective_workflow(&workflow);
    let group = resolve_group_node(&effective, group_id)?;
    let mini = build_group_workflow(&effective, group)?;

    Ok(spawn_workflow_run(
        app,
        state,
        mini,
        RunRequest {
            trigger: RunTrigger::Manual,
            delay: Duration::ZERO,
        },
    ))
}

/// 分组校验：节点必须存在且 `kind == Group`。
fn resolve_group_node<'a>(
    workflow: &'a Workflow,
    group_id: &str,
) -> Result<&'a WorkflowNode, String> {
    let node = workflow
        .nodes
        .iter()
        .find(|node| node.id == group_id)
        .ok_or_else(|| format!("分组不存在：{group_id}"))?;
    if node.kind != NodeKind::Group {
        return Err(format!("「{}」不是分组节点", graph::node_label(node)));
    }
    Ok(node)
}

/// 收集分组下所有可执行后代节点的 id（按 `workflow.nodes` 原顺序）。
///
/// 沿 `parent_id` 链展开嵌套分组；分组节点本身不进入结果。
fn collect_group_node_ids(workflow: &Workflow, group_id: &str) -> Vec<String> {
    collect_descendant_ids(&workflow.nodes, group_id)
}

/// [`collect_group_node_ids`] 的节点切片版本（有效图变换里 `nodes` 会追加合成延迟节点）。
///
/// `visited` 用于防御 `parent_id` 成环的异常数据，避免死循环。
fn collect_descendant_ids(nodes: &[WorkflowNode], group_id: &str) -> Vec<String> {
    let mut executable: HashSet<String> = HashSet::new();
    let mut pending: Vec<String> = vec![group_id.to_string()];
    let mut visited: HashSet<String> = HashSet::new();

    while let Some(parent) = pending.pop() {
        if !visited.insert(parent.clone()) {
            continue;
        }
        for node in nodes {
            if node.parent_id.as_deref() != Some(parent.as_str()) {
                continue;
            }
            if node.kind == NodeKind::Group {
                pending.push(node.id.clone());
            } else {
                executable.insert(node.id.clone());
            }
        }
    }

    nodes
        .iter()
        .filter(|node| executable.contains(&node.id))
        .map(|node| node.id.clone())
        .collect()
}

/// 构造「分组子工作流」：后代可执行节点强制启用，只保留分组内部的边。
fn build_group_workflow(workflow: &Workflow, group: &WorkflowNode) -> Result<Workflow, String> {
    let member_ids: HashSet<String> = collect_group_node_ids(workflow, &group.id)
        .into_iter()
        .collect();

    if member_ids.is_empty() {
        return Err(format!(
            "分组「{}」内没有可执行节点",
            graph::node_label(group)
        ));
    }

    let nodes: Vec<WorkflowNode> = workflow
        .nodes
        .iter()
        .filter(|node| member_ids.contains(&node.id))
        .map(|node| {
            let mut copy = node.clone();
            // 与单点执行一致：分组运行忽略节点上的禁用标记，便于调试。
            copy.enabled = true;
            copy
        })
        .collect();

    // 只保留两端都在分组内的边：内部依赖保留，跨组边丢弃。
    let edges: Vec<WorkflowEdge> = workflow
        .edges
        .iter()
        .filter(|edge| member_ids.contains(&edge.source) && member_ids.contains(&edge.target))
        .cloned()
        .collect();

    Ok(Workflow {
        nodes,
        edges,
        ..workflow.clone()
    })
}

// ---------------------------------------------------------------------------
// 有效执行图：按分组模式（Sequence / Parallel / Graph）改写运行时副本
// ---------------------------------------------------------------------------

/// 分组的有效执行模式：新数据用 `group_mode`，旧数据由 `group_parallel` 推导
/// （`true` → Parallel，`false` → Sequence）。
pub fn effective_group_mode(group: &WorkflowNode) -> GroupMode {
    match group.group_mode {
        Some(mode) => mode,
        None if group.group_parallel => GroupMode::Parallel,
        None => GroupMode::Sequence,
    }
}

/// 计算「有效执行图」：按分组模式改写运行时副本，**不修改持久化数据**。
///
/// 连线归属按「两端节点的最近公共分组」判定（见 [`lca_group`]），只有归属于某个分组的
/// 连线才受该分组模式影响，跨组/顶层连线保持原样：
/// - `Sequence`：删掉组内连线，按直接子项在 `nodes` 中的顺序串联（前一项出口 → 后一项入口，
///   success 边）；嵌套分组作为一个单元递归应用自身模式；跳过「连线延时」候选 delay 节点
///   （见 [`is_wire_delay_candidate`]）；`group_delay_ms > 0` 时在相邻单元之间插入合成
///   delay 节点（id `group-delay:<group>:<index>`，`parent_id` = 该分组）；
/// - `Parallel`：删掉组内连线（子项同时启动）；
/// - `Graph`：保持原连线。
///
/// 处理顺序自底向上（嵌套分组先处理），因此外层分组把嵌套分组当作单元时，
/// 其入口/出口已经由内层模式确定。
pub fn effective_workflow(workflow: &Workflow) -> Workflow {
    let groups: Vec<WorkflowNode> = workflow
        .nodes
        .iter()
        .filter(|node| node.kind == NodeKind::Group)
        .cloned()
        .collect();
    if groups.is_empty() {
        return workflow.clone();
    }

    // 父子关系与节点类型索引（`nodes` 追加合成节点后会同步更新）。
    let mut parent: HashMap<String, Option<String>> = workflow
        .nodes
        .iter()
        .map(|node| (node.id.clone(), node.parent_id.clone()))
        .collect();
    let mut kinds: HashMap<String, NodeKind> = workflow
        .nodes
        .iter()
        .map(|node| (node.id.clone(), node.kind))
        .collect();

    // 自底向上：嵌套深的分组先处理（稳定排序，同深度保持 nodes 顺序）。
    let mut ordered = groups;
    ordered.sort_by_key(|group| {
        std::cmp::Reverse(ancestor_group_ids(&parent, &kinds, &group.id).len())
    });

    let mut nodes: Vec<WorkflowNode> = workflow.nodes.clone();
    let mut edges: Vec<WorkflowEdge> = workflow.edges.clone();

    for group in &ordered {
        let mode = effective_group_mode(group);
        if mode == GroupMode::Graph {
            continue;
        }

        // 只移除归属于该分组的连线；跨组/顶层连线保持原样。
        edges.retain(|edge| lca_group(&parent, &kinds, edge).as_deref() != Some(group.id.as_str()));

        if mode == GroupMode::Parallel {
            continue;
        }

        // -- Sequence：直接子项按 nodes 顺序串联 --------------------------------
        // 先移除「连线延时」候选节点：它们代表连线上的延时，顺序模式由 group_delay_ms
        // 统一表达；保留它们在图上会多执行一次。只移除两条边都归属本分组的候选，
        // 避免留下悬空边。
        let wire_delay_ids: Vec<String> = nodes
            .iter()
            .filter(|node| node.parent_id.as_deref() == Some(group.id.as_str()))
            .filter(|node| is_wire_delay_candidate(workflow, node))
            .filter(|node| {
                workflow
                    .edges
                    .iter()
                    .filter(|edge| edge.source == node.id || edge.target == node.id)
                    .all(|edge| {
                        lca_group(&parent, &kinds, edge).as_deref() == Some(group.id.as_str())
                    })
            })
            .map(|node| node.id.clone())
            .collect();
        if !wire_delay_ids.is_empty() {
            nodes.retain(|node| !wire_delay_ids.contains(&node.id));
            edges.retain(|edge| {
                !wire_delay_ids.contains(&edge.source) && !wire_delay_ids.contains(&edge.target)
            });
        }

        let units: Vec<WorkflowNode> = nodes
            .iter()
            .filter(|node| node.parent_id.as_deref() == Some(group.id.as_str()))
            .cloned()
            .collect();
        if units.len() < 2 {
            continue;
        }

        for (pair_index, pair) in units.windows(2).enumerate() {
            let exits = unit_exits(&nodes, &edges, &pair[0]);
            let entries = unit_entries(&nodes, &edges, &pair[1]);

            if group.group_delay_ms > 0 {
                let synth = synth_group_delay(group, pair_index);
                let synth_id = synth.id.clone();
                parent.insert(synth_id.clone(), Some(group.id.clone()));
                kinds.insert(synth_id.clone(), NodeKind::Delay);
                nodes.push(synth);

                for exit in &exits {
                    edges.push(sequenced_edge(exit, &synth_id));
                }
                for entry in &entries {
                    edges.push(sequenced_edge(&synth_id, entry));
                }
            } else {
                for exit in &exits {
                    for entry in &entries {
                        edges.push(sequenced_edge(exit, entry));
                    }
                }
            }
        }
    }

    Workflow {
        nodes,
        edges,
        ..workflow.clone()
    }
}

/// 节点所属分组链（由近到远，只包含分组节点）。`guard` 防御 `parent_id` 成环。
fn ancestor_group_ids(
    parent: &HashMap<String, Option<String>>,
    kinds: &HashMap<String, NodeKind>,
    node_id: &str,
) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = parent.get(node_id).cloned().flatten();
    let mut guard = 0usize;
    while let Some(group_id) = current {
        if kinds.get(&group_id).copied() == Some(NodeKind::Group) {
            result.push(group_id.clone());
        }
        current = parent.get(&group_id).cloned().flatten();
        guard += 1;
        if guard > 512 {
            break;
        }
    }
    result
}

/// 连线两端节点的最近公共分组；`None` 表示跨组/顶层连线。
fn lca_group(
    parent: &HashMap<String, Option<String>>,
    kinds: &HashMap<String, NodeKind>,
    edge: &WorkflowEdge,
) -> Option<String> {
    let source_chain = ancestor_group_ids(parent, kinds, &edge.source);
    let target_chain = ancestor_group_ids(parent, kinds, &edge.target);
    source_chain
        .iter()
        .zip(target_chain.iter())
        .find(|(left, right)| left == right)
        .map(|(group_id, _)| group_id.clone())
}

/// 单元在当前边集上的入口：子树内没有「内部入边」的可执行节点。
///
/// 普通节点本身就是入口；嵌套分组取子树内无内部入边的节点（循环等异常时退回全部）。
fn unit_entries(
    nodes: &[WorkflowNode],
    edges: &[WorkflowEdge],
    unit: &WorkflowNode,
) -> Vec<String> {
    if unit.kind != NodeKind::Group {
        return vec![unit.id.clone()];
    }
    let members = collect_descendant_ids(nodes, &unit.id);
    let member_set: HashSet<&str> = members.iter().map(|id| id.as_str()).collect();
    let entries: Vec<String> = members
        .iter()
        .filter(|member| {
            !edges
                .iter()
                .any(|edge| member_set.contains(edge.source.as_str()) && &edge.target == *member)
        })
        .cloned()
        .collect();
    if entries.is_empty() {
        members
    } else {
        entries
    }
}

/// 单元在当前边集上的出口：子树内没有「内部出边」的可执行节点。
fn unit_exits(
    nodes: &[WorkflowNode],
    edges: &[WorkflowEdge],
    unit: &WorkflowNode,
) -> Vec<String> {
    if unit.kind != NodeKind::Group {
        return vec![unit.id.clone()];
    }
    let members = collect_descendant_ids(nodes, &unit.id);
    let member_set: HashSet<&str> = members.iter().map(|id| id.as_str()).collect();
    let exits: Vec<String> = members
        .iter()
        .filter(|member| {
            !edges
                .iter()
                .any(|edge| &edge.source == *member && member_set.contains(edge.target.as_str()))
        })
        .cloned()
        .collect();
    if exits.is_empty() {
        members
    } else {
        exits
    }
}

/// 合成「分组延迟」节点：只存在于运行时副本，不写回 workflow。
fn synth_group_delay(group: &WorkflowNode, index: usize) -> WorkflowNode {
    let delay_ms = group.group_delay_ms;
    WorkflowNode {
        id: format!("group-delay:{}:{}", group.id, index),
        name: format!("分组延迟 {delay_ms} ms"),
        kind: NodeKind::Delay,
        enabled: true,
        delay_ms,
        parent_id: Some(group.id.clone()),
        note: "由分组顺序模式在运行时生成".to_string(),
        ..Default::default()
    }
}

/// 顺序模式下相邻单元之间的连线（success）。
fn sequenced_edge(source: &str, target: &str) -> WorkflowEdge {
    WorkflowEdge {
        id: format!("group-seq:{source}->{target}"),
        source: source.to_string(),
        target: target.to_string(),
        condition: EdgeCondition::Success,
    }
}

/// 「连线延时」候选：组内 `kind=Delay`、恰好 1 入 1 出且两条都是 success 边。
///
/// 这类节点在原始图里代表连线上的延时；顺序模式由 `group_delay_ms` 统一控制，
/// 因此不作为顺序单元。组内独立 delay（边数不符）仍作为单元保留。
fn is_wire_delay_candidate(workflow: &Workflow, node: &WorkflowNode) -> bool {
    if node.kind != NodeKind::Delay {
        return false;
    }
    let incoming = workflow
        .edges
        .iter()
        .filter(|edge| edge.target == node.id)
        .count();
    let outgoing = workflow
        .edges
        .iter()
        .filter(|edge| edge.source == node.id)
        .count();
    if incoming != 1 || outgoing != 1 {
        return false;
    }
    workflow
        .edges
        .iter()
        .filter(|edge| edge.target == node.id || edge.source == node.id)
        .all(|edge| edge.condition == EdgeCondition::Success)
}

fn build_initial_run(workflow: &Workflow, run_id: &str, trigger: RunTrigger) -> WorkflowRun {
    let name = if workflow.name.trim().is_empty() {
        "未命名工作流".to_string()
    } else {
        workflow.name.clone()
    };
    let mut run = WorkflowRun {
        run_id: run_id.to_string(),
        workflow_id: workflow.id.clone(),
        workflow_name: name,
        trigger,
        status: RunStatus::Running,
        started_at: now_iso(),
        finished_at: None,
        message: None,
        nodes: HashMap::new(),
    };
    for node in &workflow.nodes {
        // 分组节点只用于展示，不进入运行状态表。
        if node.kind == NodeKind::Group {
            continue;
        }
        run.nodes.insert(
            node.id.clone(),
            NodeRunState {
                node_id: node.id.clone(),
                node_name: graph::node_label(node),
                status: NodeStatus::Pending,
                ..Default::default()
            },
        );
    }
    run
}

// ---------------------------------------------------------------------------
// 运行状态广播
// ---------------------------------------------------------------------------

/// 持有本次运行的完整状态；任何改动都同步到 `AppState` 并广播给前端。
struct RunSink {
    app: AppHandle,
    state: Arc<AppState>,
    run: Mutex<WorkflowRun>,
}

impl RunSink {
    fn new(app: AppHandle, state: Arc<AppState>, run: WorkflowRun) -> Self {
        Self {
            app,
            state,
            run: Mutex::new(run),
        }
    }

    fn lock_run(&self) -> std::sync::MutexGuard<'_, WorkflowRun> {
        self.run
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn snapshot(&self) -> WorkflowRun {
        self.lock_run().clone()
    }

    fn mutate(&self, change: impl FnOnce(&mut WorkflowRun)) {
        {
            let mut guard = self.lock_run();
            change(&mut guard);
        }
        let snapshot = self.snapshot();
        self.state.upsert_run(snapshot.clone());
        let _ = self.app.emit("workflow:run", &snapshot);
    }

    fn node_waiting(&self, node_id: &str) {
        self.mutate(|run| {
            if let Some(node) = run.nodes.get_mut(node_id) {
                node.status = NodeStatus::Waiting;
                node.message = Some("等待执行".to_string());
            }
        });
    }

    fn node_running(&self, node_id: &str) {
        self.mutate(|run| {
            if let Some(node) = run.nodes.get_mut(node_id) {
                node.status = NodeStatus::Running;
                node.started_at = Some(now_iso());
                node.finished_at = None;
                node.message = Some("正在执行".to_string());
                node.attempt = node.attempt.saturating_add(1);
            }
        });
    }

    fn node_finished(
        &self,
        node_id: &str,
        status: NodeStatus,
        message: impl Into<String>,
        pid: Option<u32>,
    ) {
        let message = message.into();
        self.mutate(|run| {
            if let Some(node) = run.nodes.get_mut(node_id) {
                node.status = status;
                node.message = Some(message);
                node.finished_at = Some(now_iso());
                if let Some(pid) = pid {
                    node.pid = Some(pid);
                }
            }
        });
    }

    fn finish(&self, status: RunStatus, message: impl Into<String>) {
        let message = message.into();
        self.mutate(|run| {
            run.status = status;
            run.message = Some(message);
            run.finished_at = Some(now_iso());
        });
    }

    /// 取消：所有还没结束的节点标为 Cancelled，run 标为 Cancelled。
    ///
    /// 禁用节点例外：它们本来就不会执行，保持「已禁用跳过」而不是被取消覆盖
    /// （否则前端会把「用户主动禁用」误显示成「被取消」）。
    fn cancel_unfinished(&self, disabled: &HashSet<String>) {
        self.mutate(|run| {
            for (node_id, node) in run.nodes.iter_mut() {
                if node.status.is_terminal() {
                    continue;
                }
                if disabled.contains(node_id) {
                    node.status = NodeStatus::Skipped;
                    node.message = Some("节点已禁用，已跳过".to_string());
                } else {
                    node.status = NodeStatus::Cancelled;
                    node.message = Some("运行已取消，节点未执行".to_string());
                }
                node.finished_at = Some(now_iso());
            }
            run.status = RunStatus::Cancelled;
            run.message = Some("运行已取消".to_string());
            run.finished_at = Some(now_iso());
        });
    }
}

/// 保证退出（包括 panic 展开）时一定释放取消标记，避免 `run_flag` 泄漏。
struct RunFlagGuard {
    state: Arc<AppState>,
    run_id: String,
}

impl Drop for RunFlagGuard {
    fn drop(&mut self) {
        self.state.release_run_flag(&self.run_id);
    }
}

// ---------------------------------------------------------------------------
// 内部结果
// ---------------------------------------------------------------------------

enum NodeExec {
    Success {
        message: String,
        pid: Option<u32>,
        /// 应用节点因「已在运行」而跳过启动（日志等级与普通成功不同）。
        already_running: bool,
    },
    Failed {
        message: String,
    },
    Cancelled,
}

// ---------------------------------------------------------------------------
// 调度主任务
// ---------------------------------------------------------------------------

struct PlannedNode {
    node: WorkflowNode,
    incoming: Vec<IncomingEdge>,
}

fn build_plan(workflow: &Workflow) -> Vec<PlannedNode> {
    // 分组节点不参与依赖：以它为上游的连线在依赖判定里必须被忽略，
    // 否则下游会一直等待一个永远不会产生的 outcome。
    let group_ids: HashSet<&str> = workflow
        .nodes
        .iter()
        .filter(|node| node.kind == NodeKind::Group)
        .map(|node| node.id.as_str())
        .collect();

    workflow
        .nodes
        .iter()
        // 分组节点不执行、不参与调度。
        .filter(|node| node.kind != NodeKind::Group)
        .map(|node| {
            let mut seen: HashSet<(String, EdgeCondition)> = HashSet::new();
            let mut incoming = Vec::new();
            for edge in workflow.edges.iter().filter(|edge| edge.target == node.id) {
                if group_ids.contains(edge.source.as_str()) {
                    continue;
                }
                if !seen.insert((edge.source.clone(), edge.condition)) {
                    continue;
                }
                incoming.push(IncomingEdge {
                    source: edge.source.clone(),
                    source_label: workflow
                        .node(&edge.source)
                        .map(graph::node_label)
                        .unwrap_or_else(|| edge.source.clone()),
                    condition: edge.condition,
                });
            }
            PlannedNode {
                node: node.clone(),
                incoming,
            }
        })
        .collect()
}

async fn run_task(
    app: AppHandle,
    state: Arc<AppState>,
    workflow: Workflow,
    initial: WorkflowRun,
    request: RunRequest,
    run_id: String,
    cancel: Arc<AtomicBool>,
) {
    let workflow_name = initial.workflow_name.clone();
    let sink = Arc::new(RunSink::new(app.clone(), state.clone(), initial));

    // 取消标记的释放交给 Drop guard：即使本函数 panic，也不会泄漏 run flag。
    let _flag_guard = RunFlagGuard {
        state: state.clone(),
        run_id: run_id.clone(),
    };

    // 禁用节点集合：取消收尾时它们保持「已禁用跳过」，不能被覆盖成 Cancelled。
    let disabled_ids: HashSet<String> = workflow
        .nodes
        .iter()
        .filter(|node| !node.enabled && node.kind != NodeKind::Group)
        .map(|node| node.id.clone())
        .collect();

    // 工作流级「跳过已存在进程」开关：与节点级开关按逻辑与合并后传给 launcher。
    let skip_existing_processes = workflow.skip_existing_processes;

    state.log(
        &app,
        LogLevel::Info,
        "workflow",
        format!("开始执行工作流「{workflow_name}」"),
        Some(format!(
            "runId={run_id}，触发={:?}，节点={}",
            request.trigger,
            workflow
                .nodes
                .iter()
                .filter(|node| node.kind != NodeKind::Group)
                .count()
        )),
    );

    // 登录启动的缓冲：延迟期间也响应取消。
    if !request.delay.is_zero() {
        let delay_ms = request.delay.as_millis();
        sink.mutate(|run| run.message = Some(format!("{delay_ms} 毫秒后开始执行")));
        if !sleep_cancellable(request.delay, &cancel).await {
            cancel_run_now(&app, &state, &sink, &run_id, &workflow_name, &disabled_ids);
            return;
        }
        sink.mutate(|run| run.message = None);
    }

    if workflow.nodes.is_empty() {
        sink.finish(RunStatus::Success, "工作流没有节点，无需执行");
        state.log(
            &app,
            LogLevel::Success,
            "workflow",
            format!("工作流「{workflow_name}」执行完成（没有节点）"),
            Some(format!("runId={run_id}")),
        );
        state.release_run_flag(&run_id);
        return;
    }

    let settings = state.config().settings;
    let max_concurrency = settings.max_concurrency.max(1);
    let node_timeout_ms = settings.default_node_timeout_ms;

    let planned = build_plan(&workflow);
    if planned.is_empty() {
        // 只有分组节点的极端情况（workflow.nodes 里全是 Group）。
        sink.finish(RunStatus::Success, "工作流只有分组节点，无需执行");
        state.log(
            &app,
            LogLevel::Success,
            "workflow",
            format!("工作流「{workflow_name}」执行完成（只有分组节点）"),
            Some(format!("runId={run_id}")),
        );
        state.release_run_flag(&run_id);
        return;
    }

    let mut outcomes: HashMap<String, NodeOutcome> = HashMap::new();
    let mut settled: HashSet<String> = HashSet::new();
    let mut dispatched: HashSet<String> = HashSet::new();
    let mut join_set: JoinSet<(String, NodeOutcome)> = JoinSet::new();
    let semaphore = Arc::new(Semaphore::new(max_concurrency));

    loop {
        if is_cancelled(&cancel) {
            break;
        }

        for planned_node in &planned {
            let node_id = planned_node.node.id.clone();
            if settled.contains(&node_id) || dispatched.contains(&node_id) {
                continue;
            }

            // 禁用节点：直接跳过，对下游视作成功，不阻断流程。
            if !planned_node.node.enabled {
                sink.node_finished(
                    &node_id,
                    NodeStatus::Skipped,
                    "节点已禁用，已跳过",
                    None,
                );
                state.log(
                    &app,
                    LogLevel::Info,
                    "workflow",
                    format!(
                        "节点「{}」已禁用，跳过（run {run_id}）",
                        graph::node_label(&planned_node.node)
                    ),
                    None,
                );
                outcomes.insert(node_id.clone(), NodeOutcome::Disabled);
                settled.insert(node_id);
                continue;
            }

            match evaluate_readiness(&planned_node.incoming, &outcomes) {
                Readiness::Waiting => {}
                Readiness::Blocked(reason) => {
                    sink.node_finished(&node_id, NodeStatus::Skipped, reason.clone(), None);
                    state.log(
                        &app,
                        LogLevel::Warn,
                        "workflow",
                        format!(
                            "节点「{}」已跳过（run {run_id}）",
                            graph::node_label(&planned_node.node)
                        ),
                        Some(reason),
                    );
                    outcomes.insert(node_id.clone(), NodeOutcome::Skipped);
                    settled.insert(node_id);
                }
                Readiness::Ready => {
                    dispatched.insert(node_id.clone());
                    sink.node_waiting(&node_id);

                    let task_sink = sink.clone();
                    let task_state = state.clone();
                    let task_app = app.clone();
                    let task_sem = semaphore.clone();
                    let task_cancel = cancel.clone();
                    let task_run = run_id.clone();
                    let node = planned_node.node.clone();
                    let scope = if node.kind == NodeKind::Application {
                        "launcher"
                    } else {
                        "workflow"
                    };

                    join_set.spawn(async move {
                        // 并发上限：拿到许可后才真正开始执行。
                        let permit = match task_sem.acquire_owned().await {
                            Ok(permit) => permit,
                            Err(_) => return (node_id, NodeOutcome::Cancelled),
                        };

                        if is_cancelled(&task_cancel) {
                            drop(permit);
                            return (node_id, NodeOutcome::Cancelled);
                        }

                        task_sink.node_running(&node_id);

                        let outcome = match run_node_with_timeout(
                            &node,
                            &task_cancel,
                            node_timeout_ms,
                            skip_existing_processes,
                        )
                        .await
                        {
                            NodeExec::Success {
                                message,
                                pid,
                                already_running,
                            } => {
                                // 已在运行 → SkippedExisting（界面独特颜色），对下游算成功。
                                let (status, outcome) = if already_running {
                                    (NodeStatus::SkippedExisting, NodeOutcome::SkippedExisting)
                                } else {
                                    (NodeStatus::Success, NodeOutcome::Success)
                                };
                                debug_assert!(
                                    status.is_success(),
                                    "SkippedExisting 与 Success 都属于成功终态"
                                );
                                task_sink.node_finished(&node_id, status, message.clone(), pid);
                                task_state.log(
                                    &task_app,
                                    if already_running {
                                        LogLevel::Info
                                    } else {
                                        LogLevel::Success
                                    },
                                    scope,
                                    format!("节点「{}」完成", graph::node_label(&node)),
                                    Some(format!("{message}（run {task_run}）")),
                                );
                                outcome
                            }
                            NodeExec::Failed { message } => {
                                task_sink.node_finished(
                                    &node_id,
                                    NodeStatus::Failed,
                                    message.clone(),
                                    None,
                                );
                                task_state.log(
                                    &task_app,
                                    LogLevel::Error,
                                    scope,
                                    format!("节点「{}」失败", graph::node_label(&node)),
                                    Some(format!("{message}（run {task_run}）")),
                                );
                                NodeOutcome::Failed
                            }
                            NodeExec::Cancelled => {
                                task_sink.node_finished(
                                    &node_id,
                                    NodeStatus::Cancelled,
                                    "已取消",
                                    None,
                                );
                                NodeOutcome::Cancelled
                            }
                        };

                        drop(permit);
                        (node_id, outcome)
                    });
                }
            }
        }

        if join_set.is_empty() {
            // 没有任何任务在跑：要么全部结束，要么依赖已经不可能满足。
            break;
        }

        tokio::select! {
            joined = join_set.join_next() => {
                match joined {
                    Some(Ok((node_id, outcome))) => {
                        outcomes.insert(node_id.clone(), outcome);
                        settled.insert(node_id);
                    }
                    Some(Err(err)) => {
                        state.log(
                            &app,
                            LogLevel::Error,
                            "workflow",
                            "节点任务异常终止",
                            Some(format!("{err}（run {run_id}）")),
                        );
                    }
                    None => {}
                }
            }
            _ = tokio::time::sleep(SCHEDULER_POLL_INTERVAL) => {
                // 只是为了定期回到循环顶部检查取消标记。
            }
        }
    }

    // -- 收尾 ---------------------------------------------------------------

    if is_cancelled(&cancel) {
        join_set.abort_all();
        while join_set.join_next().await.is_some() {}
        cancel_run_now(&app, &state, &sink, &run_id, &workflow_name, &disabled_ids);
        return;
    }

    // 正常路径：等待所有已调度的任务结束（此时一般已经为空）。
    while let Some(joined) = join_set.join_next().await {
        if let Ok((node_id, outcome)) = joined {
            outcomes.insert(node_id.clone(), outcome);
            settled.insert(node_id);
        }
    }

    // 理论上不会出现（DAG + 上面的调度已覆盖全部情况），兜底避免节点永远停在 Pending。
    for planned_node in &planned {
        let node_id = &planned_node.node.id;
        if !settled.contains(node_id) {
            sink.node_finished(node_id, NodeStatus::Skipped, "上游节点未完成，已跳过", None);
            outcomes.insert(node_id.clone(), NodeOutcome::Skipped);
        }
    }

    let failed = outcomes
        .values()
        .filter(|outcome| matches!(outcome, NodeOutcome::Failed))
        .count();
    let cancelled = outcomes
        .values()
        .filter(|outcome| matches!(outcome, NodeOutcome::Cancelled))
        .count();

    let (status, message, level) = if cancelled > 0 {
        (
            RunStatus::Cancelled,
            format!("有 {cancelled} 个节点被取消"),
            LogLevel::Warn,
        )
    } else if failed > 0 {
        (
            RunStatus::Failed,
            format!("{failed} 个节点执行失败"),
            LogLevel::Error,
        )
    } else {
        (
            RunStatus::Success,
            format!("全部 {} 个节点执行完成", outcomes.len()),
            LogLevel::Success,
        )
    };

    sink.finish(status, message.clone());
    state.log(
        &app,
        level,
        "workflow",
        format!("工作流「{workflow_name}」执行结束：{message}"),
        Some(format!("runId={run_id}")),
    );
    state.release_run_flag(&run_id);
}

fn cancel_run_now(
    app: &AppHandle,
    state: &Arc<AppState>,
    sink: &RunSink,
    run_id: &str,
    workflow_name: &str,
    disabled: &HashSet<String>,
) {
    sink.cancel_unfinished(disabled);
    state.log(
        app,
        LogLevel::Warn,
        "workflow",
        format!("工作流「{workflow_name}」已取消"),
        Some(format!("runId={run_id}")),
    );
    state.release_run_flag(run_id);
}

// ---------------------------------------------------------------------------
// 节点超时：按节点类型计算「有效超时」
// ---------------------------------------------------------------------------

/// 脚本自身超时之外额外给的缓冲（毫秒）。
const SCRIPT_TIMEOUT_GRACE_MS: u64 = 5_000;

/// 应用节点超时额外给的缓冲（毫秒）。
const APP_TIMEOUT_GRACE_MS: u64 = 5_000;

/// `window_timeout_ms` 为 0 时 launcher 采用的默认等待窗口超时（与 `launcher.rs` 一致）。
const APP_WINDOW_FALLBACK_MS: u64 = 15_000;

/// 期望时长的 10%，至少 5 秒。
fn timeout_buffer_ms(expected_ms: u64) -> u64 {
    (expected_ms / 10).max(5_000)
}

/// 节点实际使用的超时（毫秒；0 表示不限制，此时不包 `tokio::time::timeout`）。
///
/// 直接用 `settings.default_node_timeout_ms` 包住所有节点会把「延迟 30s+」「等待进程 60s」
/// 这类本身就有明确时长的节点误判失败，所以按节点类型分别计算：
/// - `Application`：`default_node_timeout_ms` 与「启动后预算」取较大者；启动后预算 =
///   `settle_ms + (wait_for_window ? window_timeout_ms : 0) + 5s`（`window_timeout_ms`
///   为 0 时按 launcher 的 15s 兜底算）。`default` 为 0 时仍返回 0，尊重「不限制」；
/// - `Delay`：`delay_ms + 缓冲`（缓冲 = `max(5s, delay_ms / 10)`）；
/// - `Wait`：`wait.timeout_ms + 缓冲` 与 `default_node_timeout_ms` 取较大者；
///   自身 `timeout_ms` 为 0（不设限）时退回 `default_node_timeout_ms`；
/// - `Script`：`script.timeout_ms + 5s`；脚本自身不限时退回 `default_node_timeout_ms`；
/// - `Group`：分组节点不执行，始终返回 0。
pub fn effective_timeout(node: &WorkflowNode, default_node_timeout_ms: u64) -> u64 {
    match node.kind {
        // 分组节点不参与执行，不存在超时；返回 0 只是让函数对全部 kind 有定义。
        NodeKind::Group => 0,
        NodeKind::Application => {
            if default_node_timeout_ms == 0 {
                0
            } else {
                let window_budget = if node.app.wait_for_window {
                    if node.app.window_timeout_ms == 0 {
                        APP_WINDOW_FALLBACK_MS
                    } else {
                        node.app.window_timeout_ms
                    }
                } else {
                    0
                };
                let required = node
                    .app
                    .settle_ms
                    .saturating_add(window_budget)
                    .saturating_add(APP_TIMEOUT_GRACE_MS);
                default_node_timeout_ms.max(required)
            }
        }
        NodeKind::Delay => node.delay_ms.saturating_add(timeout_buffer_ms(node.delay_ms)),
        NodeKind::Wait => {
            if node.wait.timeout_ms == 0 {
                default_node_timeout_ms
            } else {
                let own = node
                    .wait
                    .timeout_ms
                    .saturating_add(timeout_buffer_ms(node.wait.timeout_ms));
                own.max(default_node_timeout_ms)
            }
        }
        NodeKind::Script => {
            if node.script.timeout_ms > 0 {
                node.script
                    .timeout_ms
                    .saturating_add(SCRIPT_TIMEOUT_GRACE_MS)
            } else {
                default_node_timeout_ms
            }
        }
    }
}

async fn run_node_with_timeout(
    node: &WorkflowNode,
    cancel: &Arc<AtomicBool>,
    node_timeout_ms: u64,
    skip_existing_processes: bool,
) -> NodeExec {
    let effective = effective_timeout(node, node_timeout_ms);
    let execution = execute_node(node, cancel, skip_existing_processes);

    // 0 表示不限制：不包 timeout。
    if effective == 0 {
        return execution.await;
    }

    match tokio::time::timeout(Duration::from_millis(effective), execution).await {
        Ok(result) => result,
        Err(_) => {
            if is_cancelled(cancel) {
                NodeExec::Cancelled
            } else {
                NodeExec::Failed {
                    message: format!("节点执行超时（{effective} 毫秒）"),
                }
            }
        }
    }
}

async fn execute_node(
    node: &WorkflowNode,
    cancel: &Arc<AtomicBool>,
    skip_existing_processes: bool,
) -> NodeExec {
    match node.kind {
        // 分组节点不进入调度（`build_plan` 与 `build_initial_run` 都已排除），
        // 这里是穷尽 match 的防御分支：真被送进来也按「不执行、不阻断下游」处理。
        NodeKind::Group => {
            debug_assert!(false, "分组节点不应进入执行调度");
            NodeExec::Success {
                message: "分组节点不参与执行，已忽略".to_string(),
                pid: None,
                already_running: false,
            }
        }
        NodeKind::Application => {
            // 工作流级开关关闭时，无论节点怎么配置都真正执行启动。
            let skip_existing =
                skip_existing_enabled(skip_existing_processes, node.app.skip_if_running);
            // 节点名作为 display_name 传给 launcher：浏览器 PWA 靠它匹配窗口标题。
            match launcher::launch_spec(&node.app, cancel, skip_existing, &node.name).await {
                Ok(outcome) => NodeExec::Success {
                    message: outcome.message,
                    pid: outcome.pid,
                    already_running: outcome.already_running,
                },
                Err(err) => {
                    if is_cancelled(cancel) {
                        NodeExec::Cancelled
                    } else {
                        NodeExec::Failed {
                            message: err.to_string(),
                        }
                    }
                }
            }
        }
        NodeKind::Delay => execute_delay(node, cancel).await,
        NodeKind::Wait => execute_wait(node, cancel).await,
        NodeKind::Script => execute_script(&node.script, cancel).await,
    }
}

async fn execute_delay(node: &WorkflowNode, cancel: &Arc<AtomicBool>) -> NodeExec {
    if node.delay_ms == 0 {
        return NodeExec::Success {
            message: "未设置等待时间，直接通过".to_string(),
            pid: None,
            already_running: false,
        };
    }
    if sleep_cancellable(Duration::from_millis(node.delay_ms), cancel).await {
        NodeExec::Success {
            message: format!("已等待 {} 毫秒", node.delay_ms),
            pid: None,
            already_running: false,
        }
    } else {
        NodeExec::Cancelled
    }
}

async fn execute_wait(node: &WorkflowNode, cancel: &Arc<AtomicBool>) -> NodeExec {
    let spec = &node.wait;
    let process_name = spec.process_name.trim();
    if process_name.is_empty() {
        return NodeExec::Failed {
            message: "等待节点未指定进程名".to_string(),
        };
    }

    // timeout_ms 为 0 时按「几乎不限」处理（24 小时），避免立刻超时。
    let timeout_ms = if spec.timeout_ms == 0 {
        24 * 60 * 60 * 1000
    } else {
        spec.timeout_ms
    };

    // 不要求窗口可见时，「进程已经在跑」就等价于就绪：先做一次同步快检，
    // 命中就省掉后面的轮询等待（与 wait_for_window 的语义一致）。
    if !spec.window_visible && platform::is_process_running(process_name) {
        return NodeExec::Success {
            message: format!("进程「{process_name}」已就绪"),
            pid: None,
            already_running: false,
        };
    }

    let ready = platform::wait_for_window(
        process_name,
        spec.window_visible,
        Duration::from_millis(timeout_ms),
        cancel,
    )
    .await;

    if ready {
        NodeExec::Success {
            message: if spec.window_visible {
                format!("进程「{process_name}」的主窗口已出现")
            } else {
                format!("进程「{process_name}」已就绪")
            },
            pid: None,
            already_running: false,
        }
    } else if is_cancelled(cancel) {
        NodeExec::Cancelled
    } else if spec.fail_on_timeout {
        NodeExec::Failed {
            message: format!("等待进程「{process_name}」超时（{timeout_ms} 毫秒）"),
        }
    } else {
        NodeExec::Success {
            message: format!("等待进程「{process_name}」超时（{timeout_ms} 毫秒），已按配置继续"),
            pid: None,
            already_running: false,
        }
    }
}

/// 脚本节点在自身 `timeout_ms` 到期时由本函数负责 kill 子进程；
/// 外层节点超时（`effective_timeout` 会给 `script.timeout_ms + 5s`）只作为兜底，
/// 正常情况下由这里先触发，避免外层丢弃 future 后子进程残留。
async fn execute_script(spec: &ScriptSpec, cancel: &Arc<AtomicBool>) -> NodeExec {
    let command = spec.command.trim();
    if command.is_empty() {
        return NodeExec::Failed {
            message: "脚本命令为空".to_string(),
        };
    }

    let mut cmd = tokio::process::Command::new("cmd");
    cmd.arg("/C").arg(&spec.command);
    if let Some(dir) = spec
        .working_dir
        .as_deref()
        .map(str::trim)
        .filter(|dir| !dir.is_empty())
    {
        cmd.current_dir(dir);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW：避免执行脚本时弹出控制台窗口。
        cmd.as_std_mut().creation_flags(0x0800_0000);
    }
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());

    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(err) => {
            return NodeExec::Failed {
                message: format!("无法执行命令：{err}"),
            }
        }
    };

    let pid = child.id();
    let timeout_ms = spec.timeout_ms;
    let started = std::time::Instant::now();

    loop {
        if is_cancelled(cancel) {
            // 已启动的进程不强制结束。
            return NodeExec::Cancelled;
        }
        if timeout_ms > 0 && started.elapsed() >= Duration::from_millis(timeout_ms) {
            let _ = child.kill().await;
            let _ = child.wait().await;
            return NodeExec::Failed {
                message: format!("命令执行超时（{timeout_ms} 毫秒），已强制结束"),
            };
        }

        match child.try_wait() {
            Ok(Some(status)) => {
                return finish_script(status, spec.fail_on_nonzero, pid);
            }
            Ok(None) => {
                tokio::time::sleep(CANCEL_POLL_INTERVAL).await;
            }
            Err(err) => {
                return NodeExec::Failed {
                    message: format!("等待命令结束失败：{err}"),
                }
            }
        }
    }
}

fn finish_script(
    status: std::process::ExitStatus,
    fail_on_nonzero: bool,
    pid: Option<u32>,
) -> NodeExec {
    match status.code() {
        Some(0) => NodeExec::Success {
            message: "命令执行成功".to_string(),
            pid,
            already_running: false,
        },
        Some(code) if !fail_on_nonzero => NodeExec::Success {
            message: format!("命令退出码 {code}，已按配置继续"),
            pid,
            already_running: false,
        },
        Some(code) => NodeExec::Failed {
            message: format!("命令退出码 {code}"),
        },
        None => {
            if fail_on_nonzero {
                NodeExec::Failed {
                    message: "命令被终止，未取得退出码".to_string(),
                }
            } else {
                NodeExec::Success {
                    message: "命令被终止，已按配置继续".to_string(),
                    pid,
                    already_running: false,
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 单测（只覆盖不依赖 AppHandle 的纯逻辑）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{WorkflowEdge, WorkflowNode};

    fn incoming(source: &str, condition: EdgeCondition) -> IncomingEdge {
        IncomingEdge {
            source: source.to_string(),
            source_label: source.to_string(),
            condition,
        }
    }

    fn outcomes(pairs: &[(&str, NodeOutcome)]) -> HashMap<String, NodeOutcome> {
        pairs
            .iter()
            .map(|(id, outcome)| ((*id).to_string(), *outcome))
            .collect()
    }

    #[test]
    fn success_edge_only_passes_success() {
        assert!(edge_allows(EdgeCondition::Success, UpstreamOutcome::Success));
        assert!(!edge_allows(EdgeCondition::Success, UpstreamOutcome::Failed));
    }

    #[test]
    fn failure_edge_only_passes_failure() {
        assert!(edge_allows(EdgeCondition::Failure, UpstreamOutcome::Failed));
        assert!(!edge_allows(EdgeCondition::Failure, UpstreamOutcome::Success));
    }

    #[test]
    fn always_edge_passes_everything() {
        assert!(edge_allows(EdgeCondition::Always, UpstreamOutcome::Success));
        assert!(edge_allows(EdgeCondition::Always, UpstreamOutcome::Failed));
    }

    #[test]
    fn outcome_mapping_matches_scheduling_semantics() {
        assert_eq!(NodeOutcome::Success.upstream(), UpstreamOutcome::Success);
        // 已在运行跳过启动 → 对下游等同成功
        assert_eq!(
            NodeOutcome::SkippedExisting.upstream(),
            UpstreamOutcome::Success
        );
        // 禁用节点不阻断流程
        assert_eq!(NodeOutcome::Disabled.upstream(), UpstreamOutcome::Success);
        assert_eq!(NodeOutcome::Failed.upstream(), UpstreamOutcome::Failed);
        // 被跳过的节点继续沿失败方向传播
        assert_eq!(NodeOutcome::Skipped.upstream(), UpstreamOutcome::Failed);
        assert_eq!(NodeOutcome::Cancelled.upstream(), UpstreamOutcome::Failed);
    }

    #[test]
    fn skipped_existing_satisfies_success_edge_only() {
        let upstream = NodeOutcome::SkippedExisting.upstream();
        assert!(edge_allows(EdgeCondition::Success, upstream));
        assert!(!edge_allows(EdgeCondition::Failure, upstream));
        assert!(edge_allows(EdgeCondition::Always, upstream));
        // 与 model 的语义判断保持一致
        assert!(NodeStatus::SkippedExisting.is_success());
    }

    #[test]
    fn readiness_ready_after_skipped_existing_upstream() {
        let edges = vec![incoming("a", EdgeCondition::Success)];
        let result = evaluate_readiness(
            &edges,
            &outcomes(&[("a", NodeOutcome::SkippedExisting)]),
        );
        assert_eq!(result, Readiness::Ready);
    }

    #[test]
    fn skip_existing_requires_both_switches() {
        assert!(skip_existing_enabled(true, true));
        // 工作流级开关关闭 → 绝不跳过
        assert!(!skip_existing_enabled(false, true));
        assert!(!skip_existing_enabled(true, false));
        assert!(!skip_existing_enabled(false, false));
    }

    #[test]
    fn readiness_waits_for_unfinished_upstream() {
        let edges = vec![incoming("a", EdgeCondition::Success)];
        assert_eq!(evaluate_readiness(&edges, &outcomes(&[])), Readiness::Waiting);
    }

    #[test]
    fn readiness_ready_when_all_edges_satisfied() {
        let edges = vec![
            incoming("a", EdgeCondition::Success),
            incoming("b", EdgeCondition::Always),
        ];
        let result = evaluate_readiness(
            &edges,
            &outcomes(&[("a", NodeOutcome::Success), ("b", NodeOutcome::Failed)]),
        );
        assert_eq!(result, Readiness::Ready);
    }

    #[test]
    fn readiness_blocks_when_a_condition_is_impossible() {
        // 上游成功，但这条边要求「仅失败时执行」
        let edges = vec![incoming("a", EdgeCondition::Failure)];
        match evaluate_readiness(&edges, &outcomes(&[("a", NodeOutcome::Success)])) {
            Readiness::Blocked(reason) => {
                assert!(reason.contains('a'));
                assert!(reason.contains("仅失败时执行"));
            }
            other => panic!("期望 Blocked，实际 {other:?}"),
        }
    }

    #[test]
    fn readiness_requires_every_incoming_edge() {
        // a 成功（success 边满足），b 成功（failure 边不满足）→ 节点必须跳过
        let edges = vec![
            incoming("a", EdgeCondition::Success),
            incoming("b", EdgeCondition::Failure),
        ];
        let result = evaluate_readiness(
            &edges,
            &outcomes(&[("a", NodeOutcome::Success), ("b", NodeOutcome::Success)]),
        );
        assert!(matches!(result, Readiness::Blocked(_)));
    }

    #[test]
    fn readiness_blocks_early_even_if_other_upstream_pending() {
        let edges = vec![
            incoming("a", EdgeCondition::Success),
            incoming("b", EdgeCondition::Success),
        ];
        let result = evaluate_readiness(
            &edges,
            &outcomes(&[("a", NodeOutcome::Failed), ("b", NodeOutcome::Success)]),
        );
        match result {
            Readiness::Blocked(reason) => assert!(reason.contains('a')),
            other => panic!("期望 Blocked，实际 {other:?}"),
        }
    }

    fn node(id: &str) -> WorkflowNode {
        WorkflowNode {
            id: id.to_string(),
            name: id.to_string(),
            kind: NodeKind::Delay,
            ..Default::default()
        }
    }

    fn group_node(id: &str) -> WorkflowNode {
        WorkflowNode {
            id: id.to_string(),
            name: id.to_string(),
            kind: NodeKind::Group,
            ..Default::default()
        }
    }

    /// 普通节点 + 所属分组（`parent_id`）。
    fn child_node(id: &str, parent: &str) -> WorkflowNode {
        let mut node = node(id);
        node.parent_id = Some(parent.to_string());
        node
    }

    /// 嵌套分组（`parent_id` 指向外层分组）。
    fn nested_group(id: &str, parent: &str) -> WorkflowNode {
        let mut group = group_node(id);
        group.parent_id = Some(parent.to_string());
        group
    }

    /// 指定模式与组延迟的分组节点。
    fn group_with_mode(id: &str, mode: GroupMode, delay_ms: u64) -> WorkflowNode {
        let mut group = group_node(id);
        group.group_mode = Some(mode);
        group.group_delay_ms = delay_ms;
        group
    }

    fn has_edge(workflow: &Workflow, source: &str, target: &str) -> bool {
        workflow
            .edges
            .iter()
            .any(|edge| edge.source == source && edge.target == target)
    }

    fn edge(source: &str, target: &str, condition: EdgeCondition) -> WorkflowEdge {
        WorkflowEdge {
            id: format!("{source}->{target}"),
            source: source.to_string(),
            target: target.to_string(),
            condition,
        }
    }

    #[test]
    fn single_node_workflow_keeps_identity_and_forces_enabled() {
        let mut target = node("b");
        target.enabled = false;
        let workflow = Workflow {
            id: "wf-7".to_string(),
            name: "原工作流".to_string(),
            skip_existing_processes: false,
            nodes: vec![node("a"), target],
            edges: vec![edge("a", "b", EdgeCondition::Success)],
            ..Default::default()
        };

        let mini = build_single_node_workflow(&workflow, &workflow.nodes[1]);

        // 只保留目标节点，节点 id 不变（前端按 nodeId 对状态）。
        assert_eq!(mini.nodes.len(), 1);
        assert_eq!(mini.nodes[0].id, "b");
        // 调试时即使原节点被禁用也要能单独跑。
        assert!(mini.nodes[0].enabled);
        // 不带任何连线：单点执行不需要整图依赖。
        assert!(mini.edges.is_empty());
        // 身份沿用原工作流，run.workflow_id 才不会变。
        assert_eq!(mini.id, "wf-7");
        assert_eq!(mini.name, "原工作流");
        assert!(!mini.skip_existing_processes);
    }

    #[test]
    fn single_node_run_rejects_unknown_and_group_nodes() {
        let workflow = Workflow {
            id: "wf".to_string(),
            name: "示例".to_string(),
            nodes: vec![node("a"), group_node("g")],
            ..Default::default()
        };

        let ok = resolve_single_node(&workflow, "a").expect("普通节点应可单点执行");
        assert_eq!(ok.id, "a");

        let missing = resolve_single_node(&workflow, "missing").unwrap_err();
        assert!(missing.contains("节点不存在"), "实际：{missing}");

        let group = resolve_single_node(&workflow, "g").unwrap_err();
        assert!(group.contains("分组节点"), "实际：{group}");
    }

    #[test]
    fn group_collects_nested_descendants() {
        // g1 直接包含 a；g1 内还嵌套 g2，g2 包含 b；outside 不属于任何分组。
        let workflow = Workflow {
            id: "wf".to_string(),
            name: "示例".to_string(),
            nodes: vec![
                group_node("g1"),
                child_node("a", "g1"),
                nested_group("g2", "g1"),
                child_node("b", "g2"),
                node("outside"),
            ],
            ..Default::default()
        };

        let ids = collect_group_node_ids(&workflow, "g1");
        // 按 workflow.nodes 原顺序，排除 group 节点本身，递归进入嵌套分组。
        assert_eq!(ids, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn group_workflow_keeps_internal_edges_and_forces_enabled() {
        let mut disabled = child_node("a", "g1");
        disabled.enabled = false;
        let workflow = Workflow {
            id: "wf-g".to_string(),
            name: "原工作流".to_string(),
            skip_existing_processes: false,
            nodes: vec![
                group_node("g1"),
                disabled,
                child_node("b", "g1"),
                node("outside"),
            ],
            edges: vec![
                edge("a", "b", EdgeCondition::Success),       // 组内 → 保留
                edge("a", "outside", EdgeCondition::Success), // 跨组 → 丢弃
                edge("outside", "b", EdgeCondition::Success), // 跨组 → 丢弃
            ],
            ..Default::default()
        };

        let group = resolve_group_node(&workflow, "g1").expect("分组应存在");
        let mini = build_group_workflow(&workflow, group).expect("分组非空");

        // 只含组内可执行节点，且强制启用（原节点是 disabled）。
        assert_eq!(mini.nodes.len(), 2);
        assert!(mini.nodes.iter().all(|node| node.enabled));
        assert!(mini.nodes.iter().all(|node| node.kind != NodeKind::Group));
        // 只保留组内边。
        assert_eq!(mini.edges.len(), 1);
        assert_eq!(mini.edges[0].source, "a");
        assert_eq!(mini.edges[0].target, "b");
        // 身份沿用原工作流（run.workflow_id 不变）。
        assert_eq!(mini.id, "wf-g");
        assert_eq!(mini.name, "原工作流");
        assert!(!mini.skip_existing_processes);
    }

    #[test]
    fn group_run_rejects_unknown_non_group_and_empty_group() {
        let workflow = Workflow {
            id: "wf".to_string(),
            name: "示例".to_string(),
            nodes: vec![
                group_node("empty"),
                group_node("g"),
                child_node("a", "g"),
                node("plain"),
            ],
            ..Default::default()
        };

        let unknown = resolve_group_node(&workflow, "missing").unwrap_err();
        assert!(unknown.contains("分组不存在"), "实际：{unknown}");

        let not_group = resolve_group_node(&workflow, "plain").unwrap_err();
        assert!(not_group.contains("不是分组节点"), "实际：{not_group}");

        let group = resolve_group_node(&workflow, "empty").expect("空分组应存在");
        let empty = build_group_workflow(&workflow, group).unwrap_err();
        assert!(empty.contains("没有可执行节点"), "实际：{empty}");
    }

    // -- 有效执行图（分组模式） ---------------------------------------------

    #[test]
    fn sequence_ignores_inner_edges_and_chains_children() {
        // 组内 a、b、c 按 nodes 顺序；原有内部连线故意画成 c→a、a→c。
        let workflow = Workflow {
            id: "wf".to_string(),
            name: "示例".to_string(),
            nodes: vec![
                group_with_mode("g", GroupMode::Sequence, 0),
                child_node("a", "g"),
                child_node("b", "g"),
                child_node("c", "g"),
                node("outside"),
            ],
            edges: vec![
                // c 的入边 success、出边 failure → 不是「连线延时」候选，仍然是顺序单元。
                edge("c", "a", EdgeCondition::Failure),
                edge("a", "c", EdgeCondition::Success),
                edge("outside", "a", EdgeCondition::Success),
            ],
            ..Default::default()
        };

        let effective = effective_workflow(&workflow);

        // 组内连线被替换为按 nodes 顺序的串联。
        assert!(has_edge(&effective, "a", "b"));
        assert!(has_edge(&effective, "b", "c"));
        assert!(!has_edge(&effective, "c", "a"));
        assert!(!has_edge(&effective, "a", "c"));
        // 跨组边保留。
        assert!(has_edge(&effective, "outside", "a"));
        // 持久化数据未被修改。
        assert_eq!(workflow.edges.len(), 3);
    }

    #[test]
    fn sequence_inserts_synthetic_group_delay() {
        let workflow = Workflow {
            id: "wf".to_string(),
            name: "示例".to_string(),
            nodes: vec![
                group_with_mode("g", GroupMode::Sequence, 500),
                child_node("a", "g"),
                child_node("b", "g"),
            ],
            ..Default::default()
        };

        let effective = effective_workflow(&workflow);

        let synthetics: Vec<&WorkflowNode> = effective
            .nodes
            .iter()
            .filter(|node| node.id.starts_with("group-delay:"))
            .collect();
        assert_eq!(synthetics.len(), 1, "相邻两个单元之间插入一个合成延迟节点");
        let synth = synthetics[0];
        assert_eq!(synth.kind, NodeKind::Delay);
        assert_eq!(synth.delay_ms, 500);
        assert_eq!(synth.parent_id.as_deref(), Some("g"));
        assert!(synth.name.contains("分组延迟"), "名称：{}", synth.name);

        // a → 合成延迟 → b
        assert!(has_edge(&effective, "a", &synth.id));
        assert!(has_edge(&effective, &synth.id, "b"));
        assert!(!has_edge(&effective, "a", "b"));
        // 合成节点不写回原工作流。
        assert_eq!(workflow.nodes.len(), 3);
    }

    #[test]
    fn sequence_skips_wire_delay_candidates() {
        // 组内 a、d(Delay)、b，d 在原图中 1 入 1 出且都是 success → 连线延时候选，跳过。
        let mut wire_delay = child_node("d", "g");
        wire_delay.kind = NodeKind::Delay;
        wire_delay.delay_ms = 300;

        let workflow = Workflow {
            id: "wf".to_string(),
            name: "示例".to_string(),
            nodes: vec![
                group_with_mode("g", GroupMode::Sequence, 0),
                child_node("a", "g"),
                wire_delay,
                child_node("b", "g"),
            ],
            // 故意与 nodes 顺序相反，便于区分「跳过」与「串联」。
            edges: vec![
                edge("b", "d", EdgeCondition::Success),
                edge("d", "a", EdgeCondition::Success),
            ],
            ..Default::default()
        };

        let effective = effective_workflow(&workflow);

        // 连线延时候选被移除（由 group_delay_ms 统一表达），单元只剩 a、b → 直接串联。
        assert!(!effective.nodes.iter().any(|node| node.id == "d"));
        assert!(has_edge(&effective, "a", "b"));
        assert!(!has_edge(&effective, "a", "d"));
        assert!(!has_edge(&effective, "d", "b"));
        // 原工作流（持久化数据）保持不变。
        assert!(workflow.nodes.iter().any(|node| node.id == "d"));
    }

    #[test]
    fn parallel_drops_inner_edges_only() {
        let workflow = Workflow {
            id: "wf".to_string(),
            name: "示例".to_string(),
            nodes: vec![
                group_with_mode("g", GroupMode::Parallel, 0),
                child_node("a", "g"),
                child_node("b", "g"),
                node("outside"),
            ],
            edges: vec![
                edge("a", "b", EdgeCondition::Success),
                edge("outside", "a", EdgeCondition::Success),
            ],
            ..Default::default()
        };

        let effective = effective_workflow(&workflow);

        // 组内连线被忽略（子项同时启动），跨组边保留。
        assert!(!has_edge(&effective, "a", "b"));
        assert!(has_edge(&effective, "outside", "a"));
        assert_eq!(effective.nodes.len(), workflow.nodes.len());
    }

    #[test]
    fn graph_mode_keeps_original_edges() {
        let workflow = Workflow {
            id: "wf".to_string(),
            name: "示例".to_string(),
            nodes: vec![
                group_with_mode("g", GroupMode::Graph, 800),
                child_node("a", "g"),
                child_node("b", "g"),
            ],
            edges: vec![edge("b", "a", EdgeCondition::Success)],
            ..Default::default()
        };

        let effective = effective_workflow(&workflow);

        // 保持原有连线，不串联、不插入合成延迟。
        assert!(has_edge(&effective, "b", "a"));
        assert!(!has_edge(&effective, "a", "b"));
        assert_eq!(effective.edges.len(), workflow.edges.len());
        assert_eq!(effective.nodes.len(), workflow.nodes.len());
    }

    #[test]
    fn nested_group_is_one_unit_in_outer_sequence() {
        // 外层 g(Sequence)：直接子项是嵌套组 h(Sequence, 含 a、b) 和 c。
        let workflow = Workflow {
            id: "wf".to_string(),
            name: "示例".to_string(),
            nodes: vec![
                group_with_mode("g", GroupMode::Sequence, 0),
                nested_group("h", "g"),
                child_node("a", "h"),
                child_node("b", "h"),
                child_node("c", "g"),
            ],
            edges: vec![edge("a", "b", EdgeCondition::Success)],
            ..Default::default()
        };

        let effective = effective_workflow(&workflow);

        // h 内部按自身 Sequence 串联；外层把 h 当作一个单元，用 h 的出口（b）接 c。
        assert!(has_edge(&effective, "a", "b"));
        assert!(has_edge(&effective, "b", "c"));
    }

    #[test]
    fn group_mode_falls_back_to_group_parallel() {
        let mut parallel = group_node("p");
        parallel.group_parallel = true;
        parallel.group_mode = None;
        assert_eq!(effective_group_mode(&parallel), GroupMode::Parallel);

        let mut sequence = group_node("s");
        sequence.group_parallel = false;
        sequence.group_mode = None;
        assert_eq!(effective_group_mode(&sequence), GroupMode::Sequence);

        // 显式模式优先于兼容字段。
        let mut explicit = group_node("e");
        explicit.group_mode = Some(GroupMode::Graph);
        explicit.group_parallel = true;
        assert_eq!(effective_group_mode(&explicit), GroupMode::Graph);
    }

    #[test]
    fn initial_run_is_running_with_pending_nodes() {
        let workflow = Workflow {
            id: "wf".to_string(),
            name: "示例".to_string(),
            nodes: vec![node("a"), node("b")],
            ..Default::default()
        };
        let run = build_initial_run(&workflow, "run-1", RunTrigger::Manual);
        assert_eq!(run.status, RunStatus::Running);
        assert_eq!(run.nodes.len(), 2);
        assert!(run
            .nodes
            .values()
            .all(|node| node.status == NodeStatus::Pending));
    }

    #[test]
    fn initial_run_uses_fallback_name() {
        let workflow = Workflow {
            id: "wf".to_string(),
            name: "   ".to_string(),
            nodes: Vec::new(),
            ..Default::default()
        };
        let run = build_initial_run(&workflow, "run-2", RunTrigger::Startup);
        assert_eq!(run.workflow_name, "未命名工作流");
    }

    #[test]
    fn initial_run_excludes_group_nodes() {
        let workflow = Workflow {
            id: "wf".to_string(),
            name: "示例".to_string(),
            nodes: vec![node("a"), group_node("g"), node("b")],
            ..Default::default()
        };
        let run = build_initial_run(&workflow, "run-3", RunTrigger::Manual);
        // 分组节点不进入运行状态表。
        assert_eq!(run.nodes.len(), 2);
        assert!(!run.nodes.contains_key("g"));
        assert!(run.nodes.contains_key("a") && run.nodes.contains_key("b"));
    }

    #[test]
    fn plan_excludes_group_nodes() {
        let workflow = Workflow {
            id: "wf".to_string(),
            name: "示例".to_string(),
            nodes: vec![node("a"), group_node("g"), node("b")],
            edges: vec![
                edge("a", "g", EdgeCondition::Success),
                edge("g", "b", EdgeCondition::Success),
            ],
            ..Default::default()
        };
        let plan = build_plan(&workflow);
        assert_eq!(plan.len(), 2);
        assert!(plan.iter().all(|p| p.node.kind != NodeKind::Group));
        // 指向分组节点的连线不会变成依赖（连线在 graph 校验层已被忽略）。
        let b = plan.iter().find(|p| p.node.id == "b").unwrap();
        assert!(b.incoming.is_empty());
    }

    #[test]
    fn plan_collects_incoming_edges_with_conditions() {
        let workflow = Workflow {
            id: "wf".to_string(),
            name: "示例".to_string(),
            nodes: vec![node("a"), node("b"), node("c")],
            edges: vec![
                edge("a", "c", EdgeCondition::Success),
                edge("b", "c", EdgeCondition::Failure),
                edge("a", "b", EdgeCondition::Always),
            ],
            ..Default::default()
        };
        let plan = build_plan(&workflow);
        assert_eq!(plan.len(), 3);

        let find = |id: &str| plan.iter().find(|p| p.node.id == id).unwrap();
        assert!(find("a").incoming.is_empty());
        assert_eq!(find("b").incoming.len(), 1);
        assert_eq!(find("b").incoming[0].condition, EdgeCondition::Always);
        assert_eq!(find("c").incoming.len(), 2);
        assert_eq!(find("c").incoming[0].source_label, "a");
    }

    #[test]
    fn plan_deduplicates_identical_edges() {
        let workflow = Workflow {
            id: "wf".to_string(),
            name: "示例".to_string(),
            nodes: vec![node("a"), node("b")],
            edges: vec![
                edge("a", "b", EdgeCondition::Success),
                edge("a", "b", EdgeCondition::Success),
            ],
            ..Default::default()
        };
        let plan = build_plan(&workflow);
        let b = plan.iter().find(|p| p.node.id == "b").unwrap();
        assert_eq!(b.incoming.len(), 1);
    }

    #[test]
    fn effective_timeout_application_uses_default() {
        let node = WorkflowNode {
            kind: NodeKind::Application,
            ..Default::default()
        };
        assert_eq!(effective_timeout(&node, 30_000), 30_000);
        // 0 = 不限：不包 timeout。
        assert_eq!(effective_timeout(&node, 0), 0);
    }

    #[test]
    fn effective_timeout_application_covers_window_wait() {
        let mut node = WorkflowNode {
            kind: NodeKind::Application,
            ..Default::default()
        };
        node.app.wait_for_window = true;

        // 15s 窗口 + 5s 缓冲 = 20s < 默认 30s，仍取 30s。
        node.app.window_timeout_ms = 15_000;
        assert_eq!(effective_timeout(&node, 30_000), 30_000);

        // 60s 窗口 + 5s 缓冲 = 65s > 默认，取 65s，窗口等待不再被截断。
        node.app.window_timeout_ms = 60_000;
        assert_eq!(effective_timeout(&node, 30_000), 65_000);
        // 默认只有 10s 时同样取 65s。
        assert_eq!(effective_timeout(&node, 10_000), 65_000);
    }

    #[test]
    fn effective_timeout_application_uses_launcher_window_fallback() {
        let mut node = WorkflowNode {
            kind: NodeKind::Application,
            ..Default::default()
        };
        node.app.wait_for_window = true;
        // window_timeout_ms = 0 时 launcher 按 15s 等待：0 + 15s + 5s = 20s。
        node.app.window_timeout_ms = 0;
        assert_eq!(effective_timeout(&node, 10_000), 20_000);
        assert_eq!(effective_timeout(&node, 30_000), 30_000);
    }

    #[test]
    fn effective_timeout_application_without_window_wait() {
        let mut node = WorkflowNode {
            kind: NodeKind::Application,
            ..Default::default()
        };
        node.app.wait_for_window = false;

        // 只算 settle：10s + 5s 缓冲 = 15s > 默认 5s。
        node.app.settle_ms = 10_000;
        assert_eq!(effective_timeout(&node, 5_000), 15_000);
        assert_eq!(effective_timeout(&node, 30_000), 30_000);
        // 默认不限时仍不限。
        assert_eq!(effective_timeout(&node, 0), 0);
    }

    #[test]
    fn effective_timeout_delay_adds_buffer() {
        let mut node = WorkflowNode {
            kind: NodeKind::Delay,
            ..Default::default()
        };

        // 30000 + max(5000, 3000) = 35000，全局 30s 不再是硬上限。
        node.delay_ms = 30_000;
        assert_eq!(effective_timeout(&node, 30_000), 35_000);

        // 120000 + 12000 = 132000。
        node.delay_ms = 120_000;
        assert_eq!(effective_timeout(&node, 30_000), 132_000);

        // 小延迟按 5s 缓冲：1000 + 5000 = 6000。
        node.delay_ms = 1_000;
        assert_eq!(effective_timeout(&node, 30_000), 6_000);
    }

    #[test]
    fn effective_timeout_wait_uses_larger_of_own_and_default() {
        let mut node = WorkflowNode {
            kind: NodeKind::Wait,
            ..Default::default()
        };

        // 60000 + 6000 = 66000 > 默认 30000。
        node.wait.timeout_ms = 60_000;
        assert_eq!(effective_timeout(&node, 30_000), 66_000);

        // 10000 + 5000 = 15000 < 默认 30000 → 取默认，给足启动预算。
        node.wait.timeout_ms = 10_000;
        assert_eq!(effective_timeout(&node, 30_000), 30_000);

        // 默认不限时用自身计算的超时。
        assert_eq!(effective_timeout(&node, 0), 15_000);
    }

    #[test]
    fn effective_timeout_wait_without_own_timeout_falls_back_to_default() {
        let mut node = WorkflowNode {
            kind: NodeKind::Wait,
            ..Default::default()
        };
        node.wait.timeout_ms = 0;
        assert_eq!(effective_timeout(&node, 45_000), 45_000);
        assert_eq!(effective_timeout(&node, 0), 0);
    }

    #[test]
    fn effective_timeout_script_adds_grace() {
        let mut node = WorkflowNode {
            kind: NodeKind::Script,
            ..Default::default()
        };

        // 脚本自身 60s，节点兜底 65s（留 5s 给 kill 子进程）。
        node.script.timeout_ms = 60_000;
        assert_eq!(effective_timeout(&node, 30_000), 65_000);

        // 脚本不限时退回全局默认。
        node.script.timeout_ms = 0;
        assert_eq!(effective_timeout(&node, 30_000), 30_000);
        assert_eq!(effective_timeout(&node, 0), 0);
    }

    #[test]
    fn effective_timeout_zero_means_unlimited() {
        // 只有 Application / Wait / Script 在「都没设置」时会返回 0。
        let wait = WorkflowNode {
            kind: NodeKind::Wait,
            wait: crate::model::WaitSpec {
                timeout_ms: 0,
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(effective_timeout(&wait, 0), 0);

        let script = WorkflowNode {
            kind: NodeKind::Script,
            script: ScriptSpec {
                timeout_ms: 0,
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(effective_timeout(&script, 0), 0);
    }
}
