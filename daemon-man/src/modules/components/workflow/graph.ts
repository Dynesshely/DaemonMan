import type { WorkflowEdge, WorkflowNode } from "@/api";

/** 节点上下游索引（悬空边、自环一律忽略）。 */
export interface WorkflowAdjacency {
  /** nodeId -> 入边 */
  incoming: Map<string, WorkflowEdge[]>;
  /** nodeId -> 出边 */
  outgoing: Map<string, WorkflowEdge[]>;
}

/** 前端计算出的执行分层结果。 */
export interface WorkflowLayers {
  /** nodeId -> 1 起算的执行层 */
  layerOf: Map<string, number>;
  /** 每层（索引 0 = 第 1 层）的节点 id */
  layers: string[][];
  /** 层数，无节点时为 0 */
  count: number;
}

export function buildAdjacency(
  nodes: WorkflowNode[],
  edges: WorkflowEdge[],
): WorkflowAdjacency {
  const nodeIds = new Set(nodes.map((node) => node.id));
  const incoming = new Map<string, WorkflowEdge[]>();
  const outgoing = new Map<string, WorkflowEdge[]>();
  for (const node of nodes) {
    incoming.set(node.id, []);
    outgoing.set(node.id, []);
  }
  for (const edge of edges) {
    if (edge.source === edge.target) continue;
    // 悬空边（引用了不存在的节点）直接忽略，和后端引擎的容错保持一致。
    if (!nodeIds.has(edge.source) || !nodeIds.has(edge.target)) continue;
    outgoing.get(edge.source)?.push(edge);
    incoming.get(edge.target)?.push(edge);
  }
  return { incoming, outgoing };
}

/**
 * 执行分层：节点层 = 1 + max(上游层)，无上游为第 1 层。
 * 基于入度的拓扑推进，因此层号连续；成环的节点（非法结构，后端校验会拦下）
 * 统一落到第 1 层，保证前端不会出现空层或无限递增。
 */
export function computeExecutionLayers(
  nodes: WorkflowNode[],
  edges: WorkflowEdge[],
): WorkflowLayers {
  const adjacency = buildAdjacency(nodes, edges);
  const layerOf = new Map<string, number>();
  const indegree = new Map<string, number>();
  const queue: string[] = [];

  for (const node of nodes) {
    const count = adjacency.incoming.get(node.id)?.length ?? 0;
    indegree.set(node.id, count);
    if (count === 0) {
      layerOf.set(node.id, 1);
      queue.push(node.id);
    }
  }

  while (queue.length > 0) {
    const id = queue.shift() as string;
    const layer = layerOf.get(id) ?? 1;
    for (const edge of adjacency.outgoing.get(id) ?? []) {
      layerOf.set(
        edge.target,
        Math.max(layerOf.get(edge.target) ?? 1, layer + 1),
      );
      const left = (indegree.get(edge.target) ?? 1) - 1;
      indegree.set(edge.target, left);
      if (left === 0) queue.push(edge.target);
    }
  }

  // 环内节点没有入队机会，统一回落到第 1 层。
  for (const node of nodes) {
    if (!layerOf.has(node.id)) layerOf.set(node.id, 1);
  }

  const buckets = new Map<number, string[]>();
  for (const node of nodes) {
    const layer = layerOf.get(node.id) ?? 1;
    const bucket = buckets.get(layer);
    if (bucket) bucket.push(node.id);
    else buckets.set(layer, [node.id]);
  }

  const layers: string[][] = [];
  const maxLayer = Math.max(0, ...Array.from(buckets.keys()));
  for (let layer = 1; layer <= maxLayer; layer += 1) {
    layers.push(buckets.get(layer) ?? []);
  }

  return { layerOf, layers, count: layers.length };
}
