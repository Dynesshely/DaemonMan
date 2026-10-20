import type { NodePosition, WorkflowEdge, WorkflowNode } from "@/api";

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
  return pills;
}

/** 边 id -> 折叠在该边上的延时节点 id（两侧边都会映射）。 */
export function pillByEdge(
  pills: Map<string, DelayPillInfo>,
): Map<string, string> {
  const map = new Map<string, string>();
  for (const pill of pills.values()) {
    map.set(pill.sourceEdgeId, pill.nodeId);
    map.set(pill.targetEdgeId, pill.nodeId);
  }
  return map;
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

/**
 * 布局完成后把药丸放到 source / target 的中点（节点坐标是左上角，这里再偏移半个体积，
 * 让药丸中心落在两点中间）。相邻药丸链通过递归求值，成环时回退。
 */
export function placeDelayPills(
  pills: Map<string, DelayPillInfo>,
  positions: Map<string, NodePosition>,
): void {
  if (pills.size === 0) return;
  const resolving = new Set<string>();

  const resolve = (id: string): NodePosition | null => {
    const existing = positions.get(id);
    if (existing) return existing;
    const pill = pills.get(id);
    if (!pill || resolving.has(id)) return null;
    resolving.add(id);
    const source = resolve(pill.sourceId);
    const target = resolve(pill.targetId);
    resolving.delete(id);
    if (!source && !target) return null;
    const from = source ?? (target as NodePosition);
    const to = target ?? (source as NodePosition);
    const position: NodePosition = {
      x: Math.round((from.x + to.x) / 2 - DELAY_PILL_WIDTH / 2),
      y: Math.round((from.y + to.y) / 2 - DELAY_PILL_HEIGHT / 2),
    };
    positions.set(id, position);
    return position;
  };

  for (const id of pills.keys()) resolve(id);
}
