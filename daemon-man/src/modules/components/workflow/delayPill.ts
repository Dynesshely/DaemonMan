import type { WorkflowEdge, WorkflowNode } from "@/api";

import { buildAdjacency } from "./graph";

/** 连线药丸的尺寸（约 88×28，见需求）。 */
export const DELAY_PILL_WIDTH = 88;
export const DELAY_PILL_HEIGHT = 28;

/** 一个「连线延时」候选：延时节点 + 它两侧的边。 */
export interface DelayPillInfo {
  nodeId: string;
  sourceId: string;
  targetId: string;
  sourceEdgeId: string;
  targetEdgeId: string;
}

/**
 * 识别可以折叠成「连线上药丸」的延时节点：
 * - kind === "delay" 且 enabled；
 * - 恰好 1 条入边、1 条出边（悬空边/自环被 buildAdjacency 忽略）；
 * - 两条边的条件都是 success（失败/总是条件仍然保留普通卡片，避免语义歧义）。
 */
export function computeDelayPills(
  nodes: WorkflowNode[],
  edges: WorkflowEdge[],
): Map<string, DelayPillInfo> {
  const pills = new Map<string, DelayPillInfo>();
  if (nodes.length === 0) return pills;

  const adjacency = buildAdjacency(nodes, edges);
  for (const node of nodes) {
    if (node.kind !== "delay" || !node.enabled) continue;
    const incoming = adjacency.incoming.get(node.id) ?? [];
    const outgoing = adjacency.outgoing.get(node.id) ?? [];
    if (incoming.length !== 1 || outgoing.length !== 1) continue;

    const sourceEdge = incoming[0];
    const targetEdge = outgoing[0];
    if (sourceEdge.condition !== "success" || targetEdge.condition !== "success") {
      continue;
    }
    if (sourceEdge.source === node.id || targetEdge.target === node.id) continue;

    pills.set(node.id, {
      nodeId: node.id,
      sourceId: sourceEdge.source,
      targetId: targetEdge.target,
      sourceEdgeId: sourceEdge.id,
      targetEdgeId: targetEdge.id,
    });
  }

  // 相邻的两个候选（延时链）都不折叠：合成边会引用不可见节点，渲染/交互会错乱。
  // 这类延时仍然按普通卡片展示，候选判定保持动态。
  let changed = true;
  while (changed) {
    changed = false;
    for (const [id, pill] of pills) {
      if (pills.has(pill.sourceId) || pills.has(pill.targetId)) {
        pills.delete(id);
        changed = true;
      }
    }
  }
  return pills;
}

/** 合成连线 id：`delay:${sourceEdgeId}:${targetEdgeId}`。 */
export function delayEdgeId(pill: DelayPillInfo): string {
  return `delay:${pill.sourceEdgeId}:${pill.targetEdgeId}`;
}

/**
 * 渲染用的「折叠边」：每个候选延时丢弃原两条边，合成一条 source→target 的边
 * （condition 固定 success，id 稳定），其余边原样保留。
 *
 * 与 `buildDelayLayoutGraph` 的区别：这里保留所有边对象（含分组/条件等），只做 1:1 替换。
 */
export function foldDelayEdges(
  edges: WorkflowEdge[],
  pills: Map<string, DelayPillInfo>,
): WorkflowEdge[] {
  if (pills.size === 0) return edges;

  const pillBySourceEdge = new Map<string, DelayPillInfo>();
  const foldedEdgeIds = new Set<string>();
  for (const pill of pills.values()) {
    pillBySourceEdge.set(pill.sourceEdgeId, pill);
    foldedEdgeIds.add(pill.sourceEdgeId);
    foldedEdgeIds.add(pill.targetEdgeId);
  }

  const result: WorkflowEdge[] = [];
  for (const edge of edges) {
    const pill = pillBySourceEdge.get(edge.id);
    if (pill) {
      result.push({
        id: delayEdgeId(pill),
        source: pill.sourceId,
        target: pill.targetId,
        condition: "success",
      });
      continue;
    }
    if (foldedEdgeIds.has(edge.id)) continue;
    result.push(edge);
  }
  return result;
}

export interface DelayLayoutGraph {
  nodes: WorkflowNode[];
  edges: WorkflowEdge[];
}

/**
 * 为自动排列准备「折叠后」的图：
 * - 移除候选延时节点；
 * - 其两条边合并成 source→target 的虚拟边（相邻药丸链会解析到链路两端）。
 */
export function buildDelayLayoutGraph(
  nodes: WorkflowNode[],
  edges: WorkflowEdge[],
  pills: Map<string, DelayPillInfo>,
): DelayLayoutGraph {
  if (pills.size === 0) return { nodes, edges };

  const removedEdgeIds = new Set<string>();
  for (const pill of pills.values()) {
    removedEdgeIds.add(pill.sourceEdgeId);
    removedEdgeIds.add(pill.targetEdgeId);
  }

  const resolveSource = (id: string): string => {
    const seen = new Set<string>();
    let current = id;
    while (pills.has(current) && !seen.has(current)) {
      seen.add(current);
      current = pills.get(current)?.sourceId ?? current;
    }
    return current;
  };

  const resolveTarget = (id: string): string => {
    const seen = new Set<string>();
    let current = id;
    while (pills.has(current) && !seen.has(current)) {
      seen.add(current);
      current = pills.get(current)?.targetId ?? current;
    }
    return current;
  };

  const virtualEdges: WorkflowEdge[] = [];
  const seen = new Set<string>();
  for (const pill of pills.values()) {
    const source = resolveSource(pill.sourceId);
    const target = resolveTarget(pill.targetId);
    const key = `${source}->${target}`;
    if (seen.has(key)) continue;
    seen.add(key);
    virtualEdges.push({
      id: `virtual-${key}`,
      source,
      target,
      condition: "success",
    });
  }

  return {
    nodes: nodes.filter((node) => !pills.has(node.id)),
    edges: [
      ...edges.filter((edge) => !removedEdgeIds.has(edge.id)),
      ...virtualEdges,
    ],
  };
}
