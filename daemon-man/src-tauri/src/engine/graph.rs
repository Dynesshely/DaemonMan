//! 工作流图算法：环检测、拓扑排序、并行层级。
//!
//! 这里的函数全部是纯函数（不接触 Tauri / 平台层），方便单元测试。

use std::collections::{HashMap, HashSet, VecDeque};

use crate::model::{NodeKind, Workflow, WorkflowNode, WorkflowValidation};

/// 节点的展示名：优先使用名称，其次 id，最后给一个占位。
pub fn node_label(node: &WorkflowNode) -> String {
    let name = node.name.trim();
    if !name.is_empty() {
        name.to_string()
    } else if !node.id.trim().is_empty() {
        node.id.clone()
    } else {
        "（未命名节点）".to_string()
    }
}

fn label_of(workflow: &Workflow, id: &str) -> String {
    match workflow.node(id) {
        Some(node) => node_label(node),
        None => id.to_string(),
    }
}

/// 校验工作流结构，返回错误/警告以及（无环时的）拓扑顺序。
///
/// - `ok` 只在没有任何 error 时为 true；warning 不影响 `ok`。
/// - `order` 是拓扑序的节点 id；存在重复 id 或环时只包含能够确定顺序的部分。
pub fn validate(workflow: &Workflow) -> WorkflowValidation {
    let mut validation = WorkflowValidation::default();

    if workflow.nodes.is_empty() {
        validation
            .warnings
            .push("工作流没有任何节点，运行时将直接完成".to_string());
    }

    // -- 节点 id ------------------------------------------------------------
    let mut id_counts: HashMap<&str, usize> = HashMap::new();
    for node in &workflow.nodes {
        *id_counts.entry(node.id.as_str()).or_insert(0) += 1;
    }
    let mut duplicated: Vec<&str> = id_counts
        .iter()
        .filter(|(_, count)| **count > 1)
        .map(|(id, _)| *id)
        .collect();
    duplicated.sort_unstable();
    for id in &duplicated {
        let count = id_counts.get(*id).copied().unwrap_or(0);
        validation
            .errors
            .push(format!("节点 id「{id}」重复出现 {count} 次"));
    }
    for node in &workflow.nodes {
        if node.id.trim().is_empty() {
            validation
                .errors
                .push(format!("节点「{}」缺少 id", node_label(node)));
        }
    }

    // -- 节点自身字段 --------------------------------------------------------
    for node in &workflow.nodes {
        if node.kind == NodeKind::Application && node.app.path.trim().is_empty() {
            validation
                .errors
                .push(format!("应用节点「{}」未设置启动路径", node_label(node)));
        }
        if !node.enabled {
            validation.warnings.push(format!(
                "节点「{}」已禁用，执行时会被跳过",
                node_label(node)
            ));
        }
    }

    // -- 连线 ---------------------------------------------------------------
    let known_ids: HashSet<&str> = workflow.nodes.iter().map(|n| n.id.as_str()).collect();
    let mut adjacency: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut indegree: HashMap<&str, usize> =
        known_ids.iter().map(|id| (*id, 0usize)).collect();
    let mut seen_pairs: HashSet<(&str, &str)> = HashSet::new();

    for edge in &workflow.edges {
        let source_known = known_ids.contains(edge.source.as_str());
        let target_known = known_ids.contains(edge.target.as_str());

        if !source_known || !target_known {
            let mut missing = Vec::new();
            if !source_known {
                missing.push(format!("起点「{}」", edge.source));
            }
            if !target_known {
                missing.push(format!("终点「{}」", edge.target));
            }
            validation.errors.push(format!(
                "连线引用了不存在的节点：{}",
                missing.join("、")
            ));
            continue;
        }

        if edge.source == edge.target {
            validation.errors.push(format!(
                "节点「{}」存在指向自身的连线（自环）",
                label_of(workflow, &edge.source)
            ));
            continue;
        }

        // 完全重复的连线只计一次，避免入度被重复累加。
        if !seen_pairs.insert((edge.source.as_str(), edge.target.as_str())) {
            continue;
        }
        adjacency
            .entry(edge.source.as_str())
            .or_default()
            .push(edge.target.as_str());
        *indegree.entry(edge.target.as_str()).or_insert(0) += 1;
    }

    // -- 拓扑排序 / 环检测 ---------------------------------------------------
    // 重复 id 会让图本身无法解释，此时不给出拓扑序。
    if duplicated.is_empty() {
        let mut order: Vec<String> = Vec::new();
        let mut queue: VecDeque<&str> = workflow
            .nodes
            .iter()
            .filter(|node| indegree.get(node.id.as_str()).copied().unwrap_or(0) == 0)
            .map(|node| node.id.as_str())
            .collect();

        while let Some(id) = queue.pop_front() {
            order.push(id.to_string());
            if let Some(targets) = adjacency.get(id) {
                for target in targets {
                    if let Some(degree) = indegree.get_mut(target) {
                        *degree = degree.saturating_sub(1);
                        if *degree == 0 {
                            queue.push_back(target);
                        }
                    }
                }
            }
        }

        if order.len() < workflow.nodes.len() {
            let ordered: HashSet<&str> = order.iter().map(|id| id.as_str()).collect();
            let remaining: Vec<&str> = workflow
                .nodes
                .iter()
                .filter(|node| !ordered.contains(node.id.as_str()))
                .map(|node| node.id.as_str())
                .collect();

            let description = match find_cycle(&adjacency, &remaining) {
                Some(cycle) => cycle
                    .iter()
                    .map(|id| label_of(workflow, id))
                    .collect::<Vec<_>>()
                    .join(" → "),
                None => remaining
                    .iter()
                    .map(|id| label_of(workflow, id))
                    .collect::<Vec<_>>()
                    .join("、"),
            };
            validation
                .errors
                .push(format!("存在循环依赖：{description}"));
        }

        validation.order = order;
    }

    validation.ok = validation.errors.is_empty();
    validation
}

/// 沿 `adjacency` 找出一条具体的环（返回的路径首尾相同），用于错误提示。
fn find_cycle<'a>(
    adjacency: &HashMap<&'a str, Vec<&'a str>>,
    candidates: &[&'a str],
) -> Option<Vec<String>> {
    for start in candidates {
        let mut path = vec![*start];
        let mut visited: HashSet<&str> = HashSet::new();
        visited.insert(*start);
        if let Some(cycle) = dfs_cycle(start, start, adjacency, &mut path, &mut visited) {
            return Some(cycle.iter().map(|id| (*id).to_string()).collect());
        }
    }
    None
}

fn dfs_cycle<'a>(
    start: &'a str,
    current: &'a str,
    adjacency: &HashMap<&'a str, Vec<&'a str>>,
    path: &mut Vec<&'a str>,
    visited: &mut HashSet<&'a str>,
) -> Option<Vec<&'a str>> {
    for next in adjacency.get(current)? {
        if *next == start {
            let mut cycle = path.clone();
            cycle.push(start);
            return Some(cycle);
        }
        if visited.insert(next) {
            path.push(next);
            if let Some(cycle) = dfs_cycle(start, next, adjacency, path, visited) {
                return Some(cycle);
            }
            path.pop();
        }
    }
    None
}

/// 计算「并行层级」：同一层的节点之间没有依赖，可以并行启动。
///
/// 忽略引用不存在节点的连线；存在环时只返回能够排出的层级。
///
/// 目前调度器采用「动态放行」，运行时并不需要预先分层；这个函数用于并行度预览
/// 与单元测试（`#[allow(dead_code)]` 仅因非测试构建暂时没有调用方）。
#[allow(dead_code)]
pub fn levels(workflow: &Workflow) -> Vec<Vec<String>> {
    let known_ids: HashSet<&str> = workflow.nodes.iter().map(|n| n.id.as_str()).collect();
    let mut adjacency: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut indegree: HashMap<&str, usize> = known_ids.iter().map(|id| (*id, 0usize)).collect();
    let mut seen_pairs: HashSet<(&str, &str)> = HashSet::new();

    for edge in &workflow.edges {
        if edge.source == edge.target {
            continue;
        }
        if !known_ids.contains(edge.source.as_str()) || !known_ids.contains(edge.target.as_str()) {
            continue;
        }
        if !seen_pairs.insert((edge.source.as_str(), edge.target.as_str())) {
            continue;
        }
        adjacency
            .entry(edge.source.as_str())
            .or_default()
            .push(edge.target.as_str());
        *indegree.entry(edge.target.as_str()).or_insert(0) += 1;
    }

    let mut current: Vec<&str> = workflow
        .nodes
        .iter()
        .filter(|node| indegree.get(node.id.as_str()).copied().unwrap_or(0) == 0)
        .map(|node| node.id.as_str())
        .collect();

    let mut result: Vec<Vec<String>> = Vec::new();
    while !current.is_empty() {
        result.push(current.iter().map(|id| (*id).to_string()).collect());

        let mut next: Vec<&str> = Vec::new();
        for id in &current {
            if let Some(targets) = adjacency.get(*id) {
                for target in targets {
                    if let Some(degree) = indegree.get_mut(*target) {
                        *degree = degree.saturating_sub(1);
                        if *degree == 0 {
                            next.push(*target);
                        }
                    }
                }
            }
        }
        current = next;
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AppLaunchSpec, EdgeCondition, NodeKind, Workflow, WorkflowEdge, WorkflowNode};

    fn node(id: &str) -> WorkflowNode {
        WorkflowNode {
            id: id.to_string(),
            name: id.to_string(),
            kind: NodeKind::Delay,
            enabled: true,
            ..Default::default()
        }
    }

    fn app_node(id: &str, path: &str) -> WorkflowNode {
        WorkflowNode {
            id: id.to_string(),
            name: id.to_string(),
            kind: NodeKind::Application,
            app: AppLaunchSpec {
                path: path.to_string(),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    fn edge(source: &str, target: &str) -> WorkflowEdge {
        WorkflowEdge {
            id: format!("{source}->{target}"),
            source: source.to_string(),
            target: target.to_string(),
            condition: EdgeCondition::Success,
        }
    }

    fn workflow(nodes: Vec<WorkflowNode>, edges: Vec<WorkflowEdge>) -> Workflow {
        Workflow {
            id: "wf".to_string(),
            name: "测试工作流".to_string(),
            nodes,
            edges,
            ..Default::default()
        }
    }

    fn order_of(workflow: &Workflow) -> Vec<String> {
        validate(workflow).order
    }

    #[test]
    fn empty_workflow_is_warning_only() {
        let validation = validate(&workflow(Vec::new(), Vec::new()));
        assert!(validation.ok);
        assert!(validation.errors.is_empty());
        assert_eq!(validation.warnings.len(), 1);
        assert!(validation.order.is_empty());
    }

    #[test]
    fn chain_has_topological_order() {
        let wf = workflow(
            vec![node("c"), node("a"), node("b")],
            vec![edge("a", "b"), edge("b", "c")],
        );
        let validation = validate(&wf);
        assert!(validation.ok, "errors: {:?}", validation.errors);
        let order = order_of(&wf);
        assert_eq!(order.len(), 3);
        let position = |id: &str| order.iter().position(|x| x == id).unwrap();
        assert!(position("a") < position("b"));
        assert!(position("b") < position("c"));
    }

    #[test]
    fn parallel_levels_are_grouped() {
        // a → b, a → c, b → d, c → d
        let wf = workflow(
            vec![node("a"), node("b"), node("c"), node("d")],
            vec![
                edge("a", "b"),
                edge("a", "c"),
                edge("b", "d"),
                edge("c", "d"),
            ],
        );
        assert!(validate(&wf).ok);

        let groups = levels(&wf);
        assert_eq!(groups.len(), 3, "levels: {groups:?}");

        let mut sorted: Vec<Vec<String>> = groups.clone();
        for level in &mut sorted {
            level.sort();
        }
        assert_eq!(sorted[0], vec!["a"]);
        assert_eq!(sorted[1], vec!["b", "c"]);
        assert_eq!(sorted[2], vec!["d"]);
    }

    #[test]
    fn isolated_nodes_share_first_level() {
        let wf = workflow(vec![node("a"), node("b"), node("c")], Vec::new());
        let groups = levels(&wf);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].len(), 3);
        assert!(validate(&wf).ok);
    }

    #[test]
    fn cycle_is_detected_with_node_names() {
        let wf = workflow(
            vec![node("a"), node("b"), node("c")],
            vec![edge("a", "b"), edge("b", "c"), edge("c", "a")],
        );
        let validation = validate(&wf);
        assert!(!validation.ok);
        let joined = validation.errors.join("\n");
        assert!(joined.contains("循环依赖"), "errors: {joined}");
        assert!(joined.contains('a') && joined.contains('b') && joined.contains('c'));
    }

    #[test]
    fn self_loop_is_rejected() {
        let wf = workflow(vec![node("a")], vec![edge("a", "a")]);
        let validation = validate(&wf);
        assert!(!validation.ok);
        assert!(validation.errors.iter().any(|e| e.contains("自环")));
    }

    #[test]
    fn dangling_edge_is_rejected() {
        let wf = workflow(
            vec![node("a")],
            vec![edge("a", "missing"), edge("ghost", "a")],
        );
        let validation = validate(&wf);
        assert!(!validation.ok);
        assert_eq!(
            validation
                .errors
                .iter()
                .filter(|e| e.contains("不存在的节点"))
                .count(),
            2
        );
    }

    #[test]
    fn duplicate_node_id_is_rejected() {
        let wf = workflow(vec![node("a"), node("a")], Vec::new());
        let validation = validate(&wf);
        assert!(!validation.ok);
        assert!(validation.errors.iter().any(|e| e.contains("重复")));
        assert!(validation.order.is_empty());
    }

    #[test]
    fn application_node_without_path_is_rejected() {
        let wf = workflow(vec![app_node("a", "")], Vec::new());
        let validation = validate(&wf);
        assert!(!validation.ok);
        assert!(validation.errors.iter().any(|e| e.contains("启动路径")));

        let ok_wf = workflow(vec![app_node("a", r"C:\Windows\notepad.exe")], Vec::new());
        assert!(validate(&ok_wf).ok);
    }

    #[test]
    fn disabled_node_only_warns() {
        let mut disabled = node("a");
        disabled.enabled = false;
        let wf = workflow(vec![disabled, node("b")], vec![edge("a", "b")]);
        let validation = validate(&wf);
        assert!(validation.ok, "errors: {:?}", validation.errors);
        assert!(validation.warnings.iter().any(|w| w.contains("已禁用")));
        assert_eq!(order_of(&wf).len(), 2);
    }

    #[test]
    fn duplicate_edges_do_not_break_topology() {
        let wf = workflow(
            vec![node("a"), node("b")],
            vec![edge("a", "b"), edge("a", "b")],
        );
        let validation = validate(&wf);
        assert!(validation.ok);
        let order = order_of(&wf);
        assert_eq!(order.len(), 2);
        assert!(order.iter().position(|x| x == "a") < order.iter().position(|x| x == "b"));
    }

    #[test]
    fn dangling_edges_are_ignored_by_levels() {
        let wf = workflow(vec![node("a"), node("b")], vec![edge("a", "b"), edge("x", "a")]);
        let groups = levels(&wf);
        assert!(!groups.is_empty());
        assert_eq!(groups[0], vec!["a".to_string()]);
        assert_eq!(groups[1], vec!["b".to_string()]);
    }
}
