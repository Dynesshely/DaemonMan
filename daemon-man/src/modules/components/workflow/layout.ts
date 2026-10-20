import type {
  NodePosition,
  NodeStatus,
  WorkflowEdge,
  WorkflowNode,
} from "@/api";

import { computeExecutionLayers } from "./graph";

/** 节点卡片尺寸估算（实际尺寸以 Vue Flow 量测为准，这里用于布局与空分组占位）。 */
export const NODE_WIDTH = 224;
export const NODE_HEIGHT = 68;

/** 分组容器内边距 / 标题栏高度。 */
export const GROUP_PADDING = 24;
export const GROUP_HEADER = 30;

/** 空分组（还没有子项）的默认尺寸。 */
export const DEFAULT_GROUP_WIDTH = 240;
export const DEFAULT_GROUP_HEIGHT = 120;

/** 并行分组：3 列紧凑网格。 */
export const PARALLEL_COLUMNS = 3;
export const PARALLEL_GAP_X = 16;
export const PARALLEL_GAP_Y = 20;

/** 顺序分组：子项纵向紧凑排列。 */
export const SEQUENTIAL_GAP_Y = 12;

/** 自动排列时列间距 / 行间距。 */
export const LAYOUT_COLUMN_GAP = 80;
export const LAYOUT_ROW_GAP = 24;
export const LAYOUT_ORIGIN_X = 80;
export const LAYOUT_ORIGIN_Y = 60;

export interface GroupSize {
  width: number;
  height: number;
}

export interface GroupChildrenIndex {
  /** groupId -> 直接子项（按 draft 顺序） */
  childrenOf: Map<string, WorkflowNode[]>;
  /** 没有有效父分组的节点（顶层） */
  topLevel: WorkflowNode[];
  /** 所有分组节点 id */
  groupIds: Set<string>;
}

/**
 * 用 `parentId` 构建分组索引。
 * - 只认 kind === "group" 且存在的父节点，缺失/非分组父节点一律当作顶层；
 * - 自引用忽略。
 */
export function buildGroupChildren(nodes: WorkflowNode[]): GroupChildrenIndex {
  const groupIds = new Set(
    nodes.filter((node) => node.kind === "group").map((node) => node.id),
  );
  const childrenOf = new Map<string, WorkflowNode[]>();
  const topLevel: WorkflowNode[] = [];

  for (const node of nodes) {
    const parentId = node.parentId;
    if (
      parentId &&
      parentId !== node.id &&
      groupIds.has(parentId)
    ) {
      const bucket = childrenOf.get(parentId);
      if (bucket) bucket.push(node);
      else childrenOf.set(parentId, [node]);
    } else {
      topLevel.push(node);
    }
  }

  return { childrenOf, topLevel, groupIds };
}

/**
 * 分组聚合状态：failed > running > waiting > pending > cancelled > success > skipped。
 * 没有任何子项时返回 pending。
 */
export function aggregateStatus(statuses: NodeStatus[]): NodeStatus {
  if (statuses.length === 0) return "pending";
  if (statuses.includes("failed")) return "failed";
  if (statuses.includes("running")) return "running";
  if (statuses.includes("waiting")) return "waiting";
  if (statuses.includes("pending")) return "pending";
  if (statuses.includes("cancelled")) return "cancelled";
  if (statuses.includes("success")) return "success";
  return "skipped";
}

type SizeOf = (node: WorkflowNode) => GroupSize;

function leafSize(): GroupSize {
  return { width: NODE_WIDTH, height: NODE_HEIGHT };
}

function parallelMetrics(
  children: WorkflowNode[],
  sizeOf: SizeOf,
): { cellWidth: number; cellHeight: number; columns: number; rows: number } {
  let cellWidth = NODE_WIDTH;
  let cellHeight = NODE_HEIGHT;
  for (const child of children) {
    const size = sizeOf(child);
    cellWidth = Math.max(cellWidth, size.width);
    cellHeight = Math.max(cellHeight, size.height);
  }
  return {
    cellWidth,
    cellHeight,
    columns: Math.min(PARALLEL_COLUMNS, children.length),
    rows: Math.ceil(children.length / PARALLEL_COLUMNS),
  };
}

/** 递归计算分组尺寸（含嵌套分组）。 */
export function computeGroupSizes(nodes: WorkflowNode[]): Map<string, GroupSize> {
  const { childrenOf } = buildGroupChildren(nodes);
  const groups = nodes.filter((node) => node.kind === "group");
  const sizes = new Map<string, GroupSize>();

  const sizeOf: SizeOf = (node) => {
    if (node.kind !== "group") return leafSize();
    return resolve(node);
  };

  const resolving = new Set<string>();

  function resolve(group: WorkflowNode): GroupSize {
    const cached = sizes.get(group.id);
    if (cached) return cached;
    // parentId 成环（非法结构）时不再递归，回落到默认尺寸。
    if (resolving.has(group.id)) {
      return { width: DEFAULT_GROUP_WIDTH, height: DEFAULT_GROUP_HEIGHT };
    }
    resolving.add(group.id);
    const children = childrenOf.get(group.id) ?? [];
    if (children.length === 0) {
      const size: GroupSize = {
        width: DEFAULT_GROUP_WIDTH,
        height: DEFAULT_GROUP_HEIGHT,
      };
      resolving.delete(group.id);
      sizes.set(group.id, size);
      return size;
    }

    let size: GroupSize;
    if (group.groupParallel) {
      const metrics = parallelMetrics(children, sizeOf);
      size = {
        width:
          GROUP_PADDING * 2 +
          metrics.columns * metrics.cellWidth +
          Math.max(0, metrics.columns - 1) * PARALLEL_GAP_X,
        height:
          GROUP_HEADER +
          GROUP_PADDING * 2 +
          metrics.rows * metrics.cellHeight +
          Math.max(0, metrics.rows - 1) * PARALLEL_GAP_Y,
      };
    } else {
      let width = NODE_WIDTH;
      let height = 0;
      children.forEach((child, index) => {
        const childSize = sizeOf(child);
        width = Math.max(width, childSize.width);
        height += childSize.height;
        if (index > 0) height += SEQUENTIAL_GAP_Y;
      });
      size = {
        width: GROUP_PADDING * 2 + width,
        height: GROUP_HEADER + GROUP_PADDING * 2 + height,
      };
    }

    resolving.delete(group.id);
    sizes.set(group.id, size);
    return size;
  }

  for (const group of groups) {
    if (sizes.has(group.id)) continue;
    resolve(group);
  }
  return sizes;
}

/**
 * 分组感知的自动排列：
 * - 顶层单元（分组 / 未分组节点）按执行层分列、列内纵向堆叠；
 * - 顺序分组子项纵向紧凑排列；并行分组用 3 列紧凑网格；
 * - 嵌套分组递归摆放，分组宽度随列数收敛。
 *
 * 返回所有节点的目标位置（含分组本身），由调用方写回 draft/store。
 */
export function computeAutoLayout(
  nodes: WorkflowNode[],
  edges: WorkflowEdge[],
): Map<string, NodePosition> {
  const { childrenOf, topLevel } = buildGroupChildren(nodes);
  const groupSizes = computeGroupSizes(nodes);
  const { layerOf } = computeExecutionLayers(nodes, edges);
  const draftIndex = new Map(nodes.map((node, index) => [node.id, index]));

  const sizeOf: SizeOf = (node) =>
    node.kind === "group"
      ? groupSizes.get(node.id) ?? {
          width: DEFAULT_GROUP_WIDTH,
          height: DEFAULT_GROUP_HEIGHT,
        }
      : leafSize();

  const groupLayerCache = new Map<string, number>();
  function layerOfGroup(group: WorkflowNode, stack: Set<string>): number {
    const cached = groupLayerCache.get(group.id);
    if (cached !== undefined) return cached;
    if (stack.has(group.id)) return 1;
    stack.add(group.id);
    const children = childrenOf.get(group.id) ?? [];
    let layer = Number.POSITIVE_INFINITY;
    for (const child of children) {
      const childLayer =
        child.kind === "group"
          ? layerOfGroup(child, stack)
          : layerOf.get(child.id) ?? 1;
      layer = Math.min(layer, childLayer);
    }
    stack.delete(group.id);
    const resolved = Number.isFinite(layer) ? layer : 1;
    groupLayerCache.set(group.id, resolved);
    return resolved;
  }

  function unitLayer(node: WorkflowNode): number {
    if (node.kind !== "group") return layerOf.get(node.id) ?? 1;
    return layerOfGroup(node, new Set());
  }

  const positions = new Map<string, NodePosition>();

  function placeUnit(node: WorkflowNode, x: number, y: number) {
    positions.set(node.id, { x, y });
    if (node.kind !== "group") return;
    placeGroupChildren(node, x, y);
  }

  function placeGroupChildren(group: WorkflowNode, x: number, y: number) {
    const children = childrenOf.get(group.id) ?? [];
    if (children.length === 0) return;

    if (group.groupParallel) {
      const metrics = parallelMetrics(children, sizeOf);
      children.forEach((child, index) => {
        const column = index % PARALLEL_COLUMNS;
        const row = Math.floor(index / PARALLEL_COLUMNS);
        placeUnit(
          child,
          x + GROUP_PADDING + column * (metrics.cellWidth + PARALLEL_GAP_X),
          y +
            GROUP_HEADER +
            GROUP_PADDING +
            row * (metrics.cellHeight + PARALLEL_GAP_Y),
        );
      });
      return;
    }

    let offsetY = y + GROUP_HEADER + GROUP_PADDING;
    for (const child of children) {
      placeUnit(child, x + GROUP_PADDING, offsetY);
      offsetY += sizeOf(child).height + SEQUENTIAL_GAP_Y;
    }
  }

  const byLayer = new Map<number, WorkflowNode[]>();
  for (const node of topLevel) {
    const layer = unitLayer(node);
    const bucket = byLayer.get(layer);
    if (bucket) bucket.push(node);
    else byLayer.set(layer, [node]);
  }

  let columnX = LAYOUT_ORIGIN_X;
  const layers = Array.from(byLayer.keys()).sort((left, right) => left - right);
  for (const layer of layers) {
    const units = (byLayer.get(layer) ?? []).sort(
      (left, right) =>
        (draftIndex.get(left.id) ?? 0) - (draftIndex.get(right.id) ?? 0),
    );
    let columnWidth = 0;
    let cursorY = LAYOUT_ORIGIN_Y;
    for (const unit of units) {
      const size = sizeOf(unit);
      placeUnit(unit, columnX, cursorY);
      cursorY += size.height + LAYOUT_ROW_GAP;
      columnWidth = Math.max(columnWidth, size.width);
    }
    columnX += columnWidth + LAYOUT_COLUMN_GAP;
  }

  return positions;
}
