import type {
  NodePosition,
  NodeStatus,
  WorkflowEdge,
  WorkflowNode,
} from "@/api";

import { computeTopology } from "./graph";
import { buildDelayLayoutGraph, computeDelayPills } from "./delayPill";

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

/** 顺序分组：子项纵向紧凑排列（兼容旧引用）。 */
export const SEQUENTIAL_GAP_Y = 12;

/** 自动排列顶层的基础间距 / 原点。 */
export const LAYOUT_COLUMN_GAP = 80;
export const LAYOUT_ROW_GAP = 24;
export const LAYOUT_ORIGIN_X = 80;
export const LAYOUT_ORIGIN_Y = 60;

/** 思维导图：层间横向间距 / 兄弟子树纵向间距 / 多个根之间。 */
const MIND_COLUMN_GAP = 56;
const MIND_ROW_GAP = 24;
const MIND_ROOT_GAP = 40;

/** 长链换行：一行最多先按列数扫描，折行后的行间距。 */
const WRAP_ROW_GAP = 40;

/** 单棵根树的目标宽高比；落在可接受区间内就不折行。 */
const TREE_TARGET_ASPECT = 1.9;
const TREE_NO_WRAP_MIN_ASPECT = 1.3;
const TREE_NO_WRAP_MAX_ASPECT = 2.6;
/** 层数太少的短链不折行（折了反而破坏父子从左到右的直觉）。 */
const MIN_WRAP_LEVELS = 4;

/** 顶层多棵「根树」排布：行内间距 / 行间距 / 最多几行 / 目标宽高比。 */
const TOP_TREE_GAP = 56;
const TOP_ROW_GAP = 48;
const TOP_MAX_ROWS = 6;
const TARGET_ASPECT = 1.9;

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
 * 分组内部布局结果：
 * - `size` 是分组容器的外框尺寸（含 padding / 标题栏）；
 * - `children` 是直接子项相对分组左上角的位置。
 */
export interface GroupPlan {
  size: GroupSize;
  children: Map<string, NodePosition>;
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
    if (parentId && parentId !== node.id && groupIds.has(parentId)) {
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
type Compare = (left: WorkflowNode, right: WorkflowNode) => number;

function leafSize(): GroupSize {
  return { width: NODE_WIDTH, height: NODE_HEIGHT };
}

function emptyPlan(): GroupPlan {
  return {
    size: { width: DEFAULT_GROUP_WIDTH, height: DEFAULT_GROUP_HEIGHT },
    children: new Map(),
  };
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

/** 并行分组：保持 3 列紧凑网格。 */
function planParallelGroup(
  children: WorkflowNode[],
  sizeOf: SizeOf,
): GroupPlan {
  const metrics = parallelMetrics(children, sizeOf);
  const positions = new Map<string, NodePosition>();
  children.forEach((child, index) => {
    positions.set(child.id, {
      x:
        GROUP_PADDING +
        (index % PARALLEL_COLUMNS) * (metrics.cellWidth + PARALLEL_GAP_X),
      y:
        GROUP_HEADER +
        GROUP_PADDING +
        Math.floor(index / PARALLEL_COLUMNS) * (metrics.cellHeight + PARALLEL_GAP_Y),
    });
  });
  return {
    size: {
      width:
        GROUP_PADDING * 2 +
        metrics.columns * metrics.cellWidth +
        Math.max(0, metrics.columns - 1) * PARALLEL_GAP_X,
      height:
        GROUP_HEADER +
        GROUP_PADDING * 2 +
        metrics.rows * metrics.cellHeight +
        Math.max(0, metrics.rows - 1) * PARALLEL_GAP_Y,
    },
    children: positions,
  };
}

interface LocalGraph {
  levelOf: Map<string, number>;
  predecessors: Map<string, string[]>;
  successors: Map<string, string[]>;
  order: string[];
}

/** 在「单元」这张局部图上做拓扑分层（去重、忽略自环）。 */
function buildLocalGraph(
  units: WorkflowNode[],
  pairs: Array<[string, string]>,
  compare: Compare,
): LocalGraph {
  const predecessors = new Map<string, string[]>();
  const successors = new Map<string, string[]>();
  for (const unit of units) {
    predecessors.set(unit.id, []);
    successors.set(unit.id, []);
  }

  const seen = new Set<string>();
  for (const [source, target] of pairs) {
    if (source === target) continue;
    if (!successors.has(source) || !predecessors.has(target)) continue;
    const key = `${source}->${target}`;
    if (seen.has(key)) continue;
    seen.add(key);
    successors.get(source)?.push(target);
    predecessors.get(target)?.push(source);
  }

  const indegree = new Map<string, number>();
  for (const unit of units) {
    indegree.set(unit.id, predecessors.get(unit.id)?.length ?? 0);
  }

  const orderByNode = new Map<string, number>();
  units.forEach((unit, index) => orderByNode.set(unit.id, index));
  const compareIds = (left: string, right: string) =>
    (orderByNode.get(left) ?? 0) - (orderByNode.get(right) ?? 0);

  const ready = units
    .filter((unit) => (indegree.get(unit.id) ?? 0) === 0)
    .sort(compare)
    .map((unit) => unit.id);
  const levelOf = new Map<string, number>();
  ready.forEach((id) => levelOf.set(id, 1));

  const order: string[] = [];
  while (ready.length > 0) {
    const id = ready.shift() as string;
    order.push(id);
    const level = levelOf.get(id) ?? 1;
    for (const nextId of successors.get(id) ?? []) {
      levelOf.set(nextId, Math.max(levelOf.get(nextId) ?? 1, level + 1));
      const left = (indegree.get(nextId) ?? 1) - 1;
      indegree.set(nextId, left);
      if (left > 0) continue;
      let at = ready.length;
      for (let index = 0; index < ready.length; index += 1) {
        if (compareIds(nextId, ready[index]) < 0) {
          at = index;
          break;
        }
      }
      ready.splice(at, 0, nextId);
    }
  }

  // 成环等非法结构：补到第 1 层，保证布局不丢节点。
  for (const unit of [...units].sort(compare)) {
    if (levelOf.has(unit.id)) continue;
    levelOf.set(unit.id, 1);
    order.push(unit.id);
  }

  return { levelOf, predecessors, successors, order };
}

/** 把后代节点映射到「直接子项单元」，用于把深层连线折叠成本层依赖。 */
function mapDescendantsToUnits(
  units: WorkflowNode[],
  childrenOf: Map<string, WorkflowNode[]>,
): Map<string, string> {
  const unitOfNode = new Map<string, string>();
  const mark = (unit: WorkflowNode, rootId: string) => {
    for (const child of childrenOf.get(unit.id) ?? []) {
      unitOfNode.set(child.id, rootId);
      mark(child, rootId);
    }
  };
  for (const unit of units) {
    unitOfNode.set(unit.id, unit.id);
    mark(unit, unit.id);
  }
  return unitOfNode;
}

interface ForestTree {
  rootId: string;
  /** 该树所有节点中最左的 x（用于行内整体对齐） */
  minX: number;
  width: number;
  height: number;
  nodes: string[];
  /** 折行后的树内相对坐标（未折行时为 null，直接沿用全局 positions） */
  wrapped: Map<string, NodePosition> | null;
}

interface WrapChunk {
  levels: number[];
  ids: string[];
  start: number;
  end: number;
  minY: number;
  height: number;
}

/**
 * 长链换行：思维导图在单条超长链上会横向铺到很宽，这里把「层列」按每行 N 列折成多行，
 * 奇数行做水平镜像（蛇形），行内保留原来的上下相对位置，行与行顶部对齐。
 * 逐列数方案比较整体宽高比，取最接近 TREE_TARGET_ASPECT 的折法；本来就够紧凑则不动。
 */
function wrapTreeColumns(
  nodeIds: string[],
  positions: Map<string, NodePosition>,
  levelOf: Map<string, number>,
  sizeById: (id: string) => GroupSize,
  columnX: Map<number, number>,
  widthByLevel: Map<number, number>,
): { width: number; height: number; positions: Map<string, NodePosition> } | null {
  const byLevel = new Map<number, string[]>();
  for (const id of nodeIds) {
    const level = levelOf.get(id) ?? 1;
    const bucket = byLevel.get(level);
    if (bucket) bucket.push(id);
    else byLevel.set(level, [id]);
  }
  const levels = [...byLevel.keys()].sort((a, b) => a - b);
  // 短链（<4 层）不折行会破坏「父左子右」的直觉；但单链（每层只有 1 个）例外：
  // 长单链折成多行很自然，继续横向铺才是真的超宽。
  const simpleChain = [...byLevel.values()].every((ids) => ids.length === 1);
  if (levels.length < MIN_WRAP_LEVELS && !(simpleChain && levels.length >= 3)) {
    return null;
  }

  const columnStart = (level: number) => columnX.get(level) ?? 0;
  const columnEnd = (level: number) =>
    columnStart(level) + (widthByLevel.get(level) ?? NODE_WIDTH);

  const buildChunks = (columnsPerRow: number): WrapChunk[] => {
    const chunks: WrapChunk[] = [];
    for (let index = 0; index < levels.length; index += columnsPerRow) {
      const chunkLevels = levels.slice(index, index + columnsPerRow);
      const ids: string[] = [];
      for (const level of chunkLevels) ids.push(...(byLevel.get(level) ?? []));
      let minY = Number.POSITIVE_INFINITY;
      let maxY = Number.NEGATIVE_INFINITY;
      for (const id of ids) {
        const position = positions.get(id);
        if (!position) continue;
        minY = Math.min(minY, position.y);
        maxY = Math.max(maxY, position.y + sizeById(id).height);
      }
      const hasBounds = Number.isFinite(minY);
      chunks.push({
        levels: chunkLevels,
        ids,
        start: columnStart(chunkLevels[0]),
        end: columnEnd(chunkLevels[chunkLevels.length - 1]),
        minY: hasBounds ? minY : 0,
        height: hasBounds ? maxY - minY : 0,
      });
    }
    return chunks;
  };

  const measure = (chunks: WrapChunk[]) => {
    let width = 0;
    let height = 0;
    chunks.forEach((chunk, index) => {
      width = Math.max(width, chunk.end - chunk.start);
      if (index > 0) height += WRAP_ROW_GAP;
      height += chunk.height;
    });
    return { width, height };
  };

  // 本来就接近目标比例：不折行，保持思维导图形态。
  const noWrapChunks = buildChunks(levels.length);
  const noWrap = measure(noWrapChunks);
  const noWrapAspect = noWrap.height > 0 ? noWrap.width / noWrap.height : Number.POSITIVE_INFINITY;
  if (
    noWrapAspect >= TREE_NO_WRAP_MIN_ASPECT &&
    noWrapAspect <= TREE_NO_WRAP_MAX_ASPECT
  ) {
    return null;
  }

  let bestChunks = noWrapChunks;
  let bestScore = Number.POSITIVE_INFINITY;
  for (let columns = levels.length; columns >= 1; columns -= 1) {
    const chunks = buildChunks(columns);
    const bounds = measure(chunks);
    const aspect = bounds.height > 0
      ? bounds.width / bounds.height
      : Number.POSITIVE_INFINITY;
    // 越接近目标比例越好；折行越多给一点点惩罚，避免无谓碎片化。
    const score =
      Math.abs(aspect - TREE_TARGET_ASPECT) +
      (levels.length - columns) * 0.01;
    if (score < bestScore) {
      bestScore = score;
      bestChunks = chunks;
    }
  }
  if (bestChunks.length <= 1) return null;

  const wrapped = new Map<string, NodePosition>();
  const bounds = measure(bestChunks);
  let cursorY = 0;
  bestChunks.forEach((chunk, index) => {
    const chunkWidth = chunk.end - chunk.start;
    const mirror = index % 2 === 1;
    for (const id of chunk.ids) {
      const position = positions.get(id);
      if (!position) continue;
      const width = sizeById(id).width;
      let x = position.x - chunk.start;
      if (mirror) x = chunkWidth - x - width;
      wrapped.set(id, { x, y: cursorY + (position.y - chunk.minY) });
    }
    cursorY += chunk.height + WRAP_ROW_GAP;
  });

  return {
    width: bounds.width,
    height: Math.max(0, cursorY - WRAP_ROW_GAP),
    positions: wrapped,
  };
}

interface ForestLayout {
  /** 单元 -> 位置（每棵树内部都以自身左上角为参考，外部按 tree 边界平移） */
  positions: Map<string, NodePosition>;
  trees: ForestTree[];
  levelOf: Map<string, number>;
}

/**
 * 把一组「单元」按拓扑展开成思维导图森林：
 * - 父节点在左，子节点向右按层推进；
 * - 同一父节点的多个子节点纵向展开，父节点垂直居中对齐子节点范围；
 * - 兄弟子树互不重叠；
 * - 每个根及其子树独立成 tree，交由调用方决定「树之间」怎么排。
 */
function layoutUnitForest(
  units: WorkflowNode[],
  pairs: Array<[string, string]>,
  sizeOf: SizeOf,
  compare: Compare,
): ForestLayout {
  const graph = buildLocalGraph(units, pairs, compare);
  const byId = new Map(units.map((unit) => [unit.id, unit]));
  const orderIndex = new Map(units.map((unit, index) => [unit.id, index]));
  const compareIds = (left: string, right: string) =>
    (orderIndex.get(left) ?? 0) - (orderIndex.get(right) ?? 0);

  // 每个单元只挂一个「主父节点」，其余连线仍然保留（只是不参与布局分层）。
  const primaryParent = new Map<string, string>();
  for (const unit of units) {
    const predecessors = graph.predecessors.get(unit.id) ?? [];
    if (predecessors.length === 0) continue;
    let best = predecessors[0];
    for (const candidate of predecessors) {
      const candidateLevel = graph.levelOf.get(candidate) ?? 1;
      const bestLevel = graph.levelOf.get(best) ?? 1;
      if (
        candidateLevel > bestLevel ||
        (candidateLevel === bestLevel && compareIds(candidate, best) < 0)
      ) {
        best = candidate;
      }
    }
    primaryParent.set(unit.id, best);
  }

  // 主父关系成环时断开，保证森林覆盖所有单元。
  for (const unit of units) {
    const chain = new Set<string>([unit.id]);
    let current = primaryParent.get(unit.id);
    while (current) {
      if (chain.has(current)) {
        primaryParent.delete(unit.id);
        break;
      }
      chain.add(current);
      current = primaryParent.get(current);
    }
  }

  const childrenByParent = new Map<string, string[]>();
  for (const [childId, parentId] of primaryParent) {
    const list = childrenByParent.get(parentId);
    if (list) list.push(childId);
    else childrenByParent.set(parentId, [childId]);
  }

  // 自底向上算每棵子树的高度。
  const subtreeHeight = new Map<string, number>();
  const computing = new Set<string>();
  const heightOf = (id: string): number => {
    const cached = subtreeHeight.get(id);
    if (cached !== undefined) return cached;
    const unit = byId.get(id);
    if (!unit) return NODE_HEIGHT;
    if (computing.has(id)) return sizeOf(unit).height;
    computing.add(id);
    const kids = [...(childrenByParent.get(id) ?? [])].sort(compareIds);
    let content = 0;
    kids.forEach((kid, index) => {
      content += heightOf(kid);
      if (index > 0) content += MIND_ROW_GAP;
    });
    const height = Math.max(sizeOf(unit).height, content);
    computing.delete(id);
    subtreeHeight.set(id, height);
    return height;
  };
  for (const unit of units) heightOf(unit.id);

  const positions = new Map<string, NodePosition>();
  const place = (
    id: string,
    x: number,
    top: number,
    columnX: Map<number, number>,
  ) => {
    const unit = byId.get(id);
    if (!unit) return;
    const height = subtreeHeight.get(id) ?? sizeOf(unit).height;
    const unitHeight = sizeOf(unit).height;
    const kids = [...(childrenByParent.get(id) ?? [])].sort(compareIds);

    if (kids.length === 0) {
      positions.set(id, { x, y: top + (height - unitHeight) / 2 });
      return;
    }

    let contentHeight = 0;
    kids.forEach((kid, index) => {
      contentHeight += heightOf(kid);
      if (index > 0) contentHeight += MIND_ROW_GAP;
    });

    let cursorY = top + Math.max(0, (height - contentHeight) / 2);
    let firstChildTop = cursorY;
    let lastChildBottom = cursorY;
    kids.forEach((kid, index) => {
      const kidHeight = heightOf(kid);
      if (index === 0) firstChildTop = cursorY;
      const kidLevel = graph.levelOf.get(kid) ?? 1;
      const kidX = columnX.get(kidLevel) ?? x + NODE_WIDTH + MIND_COLUMN_GAP;
      place(kid, kidX, cursorY, columnX);
      lastChildBottom = cursorY + kidHeight;
      cursorY += kidHeight + MIND_ROW_GAP;
    });

    // 父节点垂直居中对齐子节点范围（并夹在自身子树高度内）。
    const center = (firstChildTop + lastChildBottom) / 2;
    let y = center - unitHeight / 2;
    y = Math.max(top, Math.min(y, top + height - unitHeight));
    positions.set(id, { x, y });
  };

  const collectSubtree = (rootId: string): string[] => {
    const result: string[] = [];
    const seen = new Set<string>();
    const stack = [rootId];
    while (stack.length > 0) {
      const id = stack.pop() as string;
      if (seen.has(id)) continue;
      seen.add(id);
      result.push(id);
      stack.push(...(childrenByParent.get(id) ?? []));
    }
    return result;
  };

  const roots = units
    .filter((unit) => !primaryParent.has(unit.id))
    .sort(compare);
  const trees: ForestTree[] = [];
  for (const root of roots) {
    const nodes = collectSubtree(root.id);
    // 每棵树用自己的列宽（不与其它树的宽单元互相撑开）。
    const widthByLevel = new Map<number, number>();
    for (const id of nodes) {
      const unit = byId.get(id);
      if (!unit) continue;
      const level = graph.levelOf.get(id) ?? 1;
      widthByLevel.set(
        level,
        Math.max(widthByLevel.get(level) ?? 0, sizeOf(unit).width),
      );
    }
    const treeColumnX = new Map<number, number>();
    let columnCursor = 0;
    for (const level of Array.from(widthByLevel.keys()).sort((a, b) => a - b)) {
      treeColumnX.set(level, columnCursor);
      columnCursor += (widthByLevel.get(level) ?? NODE_WIDTH) + MIND_COLUMN_GAP;
    }
    const rootLevel = graph.levelOf.get(root.id) ?? 1;
    place(root.id, treeColumnX.get(rootLevel) ?? 0, 0, treeColumnX);

    const sizeById = (id: string): GroupSize => {
      const unit = byId.get(id);
      return unit ? sizeOf(unit) : leafSize();
    };
    const wrapped = wrapTreeColumns(
      nodes,
      positions,
      graph.levelOf,
      sizeById,
      treeColumnX,
      widthByLevel,
    );
    if (wrapped) {
      for (const [id, position] of wrapped.positions) {
        positions.set(id, position);
      }
      trees.push({
        rootId: root.id,
        minX: 0,
        width: wrapped.width,
        height: wrapped.height,
        nodes,
        wrapped: wrapped.positions,
      });
      continue;
    }

    let minX = Number.POSITIVE_INFINITY;
    let maxX = Number.NEGATIVE_INFINITY;
    for (const id of nodes) {
      const position = positions.get(id);
      const node = byId.get(id);
      if (!position || !node) continue;
      minX = Math.min(minX, position.x);
      maxX = Math.max(maxX, position.x + sizeOf(node).width);
    }
    trees.push({
      rootId: root.id,
      minX: Number.isFinite(minX) ? minX : 0,
      width: Math.max(0, maxX - (Number.isFinite(minX) ? minX : 0)),
      height: heightOf(root.id),
      nodes,
      wrapped: null,
    });
  }

  return { positions, trees, levelOf: graph.levelOf };
}

/**
 * 顺序分组：按子图拓扑做思维导图展开。
 * 组内多棵互不相连的子树纵向堆叠（组是紧凑容器），并保持从左到右。
 */
function planSequentialGroup(
  children: WorkflowNode[],
  edges: WorkflowEdge[],
  childrenOf: Map<string, WorkflowNode[]>,
  sizeOf: SizeOf,
  compare: Compare,
): GroupPlan {
  const units = [...children].sort(compare);
  if (units.length === 0) return emptyPlan();

  const unitOfNode = mapDescendantsToUnits(units, childrenOf);
  const pairs: Array<[string, string]> = [];
  for (const edge of edges) {
    const source = unitOfNode.get(edge.source);
    const target = unitOfNode.get(edge.target);
    if (!source || !target || source === target) continue;
    pairs.push([source, target]);
  }

  const forest = layoutUnitForest(units, pairs, sizeOf, compare);
  const positions = new Map<string, NodePosition>();
  let offsetY = 0;
  let contentWidth = 0;

  for (const tree of forest.trees) {
    for (const id of tree.nodes) {
      const position = forest.positions.get(id);
      if (!position) continue;
      positions.set(id, {
        x: position.x - tree.minX,
        y: position.y + offsetY,
      });
    }
    contentWidth = Math.max(contentWidth, tree.width);
    offsetY += tree.height + MIND_ROOT_GAP;
  }
  const contentHeight = Math.max(0, offsetY - MIND_ROOT_GAP);

  const childrenPositions = new Map<string, NodePosition>();
  for (const [id, position] of positions) {
    childrenPositions.set(id, {
      x: Math.round(GROUP_PADDING + position.x),
      y: Math.round(GROUP_HEADER + GROUP_PADDING + position.y),
    });
  }

  return {
    size: {
      width: Math.round(GROUP_PADDING * 2 + Math.max(contentWidth, NODE_WIDTH)),
      height: Math.round(
        GROUP_HEADER + GROUP_PADDING * 2 + Math.max(contentHeight, NODE_HEIGHT),
      ),
    },
    children: childrenPositions,
  };
}

/**
 * 计算所有分组的内部布局计划（递归处理嵌套分组）。
 * 分组尺寸与子项位置来自同一份计算，保证容器边界和内部内容严格一致。
 */
export function computeGroupPlans(
  nodes: WorkflowNode[],
  edges: WorkflowEdge[],
): Map<string, GroupPlan> {
  const { childrenOf } = buildGroupChildren(nodes);
  const topology = computeTopology(nodes, edges);
  const stepOf = topology.stepOf;
  const draftIndex = new Map(nodes.map((node, index) => [node.id, index]));
  const compare: Compare = (left, right) =>
    (stepOf.get(left.id) ?? Number.MAX_SAFE_INTEGER) -
      (stepOf.get(right.id) ?? Number.MAX_SAFE_INTEGER) ||
    (draftIndex.get(left.id) ?? 0) - (draftIndex.get(right.id) ?? 0);

  const plans = new Map<string, GroupPlan>();
  const resolving = new Set<string>();

  const sizeOf: SizeOf = (node) =>
    node.kind === "group" ? resolve(node).size : leafSize();

  function resolve(group: WorkflowNode): GroupPlan {
    const cached = plans.get(group.id);
    if (cached) return cached;
    // parentId 成环（非法结构）时回落到默认尺寸。
    if (resolving.has(group.id)) return emptyPlan();

    resolving.add(group.id);
    const children = [...(childrenOf.get(group.id) ?? [])].sort(compare);
    let plan: GroupPlan;
    if (children.length === 0) {
      plan = emptyPlan();
    } else if (group.groupParallel) {
      plan = planParallelGroup(children, sizeOf);
    } else {
      plan = planSequentialGroup(children, edges, childrenOf, sizeOf, compare);
    }
    resolving.delete(group.id);
    plans.set(group.id, plan);
    return plan;
  }

  for (const node of nodes) {
    if (node.kind === "group") resolve(node);
  }
  return plans;
}

/** 只取尺寸（画布需要根据分组计划更新容器外框）。 */
export function computeGroupSizes(
  nodes: WorkflowNode[],
  edges: WorkflowEdge[] = [],
): Map<string, GroupSize> {
  const sizes = new Map<string, GroupSize>();
  for (const [id, plan] of computeGroupPlans(nodes, edges)) {
    sizes.set(id, plan.size);
  }
  return sizes;
}

/**
 * 自动排列（landscape 思维导图）：
 * - 整图按拓扑拆成若干「根树」，每棵树内部是思维导图（父左、子右、兄弟纵向展开）；
 * - 多棵根树在顶层水平排开（必要时换行，最多 3 行），避免一列到底的竖长图；
 * - 分组内部同样按子图拓扑展开；并行分组仍是 3 列紧凑网格。
 *
 * 候选「连线延时」会先从布局图中折叠掉（两条边合并为 source→target 虚拟边）；
 * 它们不再产出坐标，药丸改由合成边上的 EdgeLabelRenderer 渲染（必然在连线上）。
 *
 * 返回所有可见节点的目标位置（含分组），由调用方写回 draft/store。
 */
export function computeAutoLayout(
  nodes: WorkflowNode[],
  edges: WorkflowEdge[],
): Map<string, NodePosition> {
  const pills = computeDelayPills(nodes, edges);
  if (pills.size === 0) return computeAutoLayoutCore(nodes, edges);
  // 候选延时（连线标签）先从布局图折叠掉，不再产出它们的坐标：
  // 药丸由合成边上的 EdgeLabelRenderer 渲染，位置必然在连线上。
  const graph = buildDelayLayoutGraph(nodes, edges, pills);
  return computeAutoLayoutCore(graph.nodes, graph.edges);
}

function computeAutoLayoutCore(
  nodes: WorkflowNode[],
  edges: WorkflowEdge[],
): Map<string, NodePosition> {
  const { childrenOf, topLevel } = buildGroupChildren(nodes);
  const plans = computeGroupPlans(nodes, edges);
  const topology = computeTopology(nodes, edges);
  const layerOf = topology.layerOf;
  const stepOf = topology.stepOf;
  const draftIndex = new Map(nodes.map((node, index) => [node.id, index]));
  const nodeById = new Map(nodes.map((node) => [node.id, node]));

  const compare: Compare = (left, right) =>
    (layerOf.get(left.id) ?? 1) - (layerOf.get(right.id) ?? 1) ||
    (stepOf.get(left.id) ?? Number.MAX_SAFE_INTEGER) -
      (stepOf.get(right.id) ?? Number.MAX_SAFE_INTEGER) ||
    (draftIndex.get(left.id) ?? 0) - (draftIndex.get(right.id) ?? 0);

  const sizeOf: SizeOf = (node) =>
    node.kind === "group"
      ? plans.get(node.id)?.size ?? {
          width: DEFAULT_GROUP_WIDTH,
          height: DEFAULT_GROUP_HEIGHT,
        }
      : leafSize();

  // 顶层单元 = 顶层节点（分组或未分组节点）；后代折叠到其顶层祖先。
  const unitOfNode = mapDescendantsToUnits(topLevel, childrenOf);
  const pairs: Array<[string, string]> = [];
  for (const edge of edges) {
    const source = unitOfNode.get(edge.source);
    const target = unitOfNode.get(edge.target);
    if (!source || !target || source === target) continue;
    pairs.push([source, target]);
  }

  const forest = layoutUnitForest(topLevel, pairs, sizeOf, compare);

  // 多棵根树排布：逐行数（1..6）尝试「均分换行」，取整体宽高比最接近目标的方案。
  // 单棵树已经过列折行，这里主要处理多棵树/多分组的横向拼排。
  const trees = forest.trees;

  const packBalanced = (rowCount: number): ForestTree[][] => {
    // 均分行数：每行树数量尽量相同，避免贪心在最后一行堆成超宽行。
    const rows: ForestTree[][] = [];
    const base = Math.floor(trees.length / rowCount);
    const extra = trees.length % rowCount;
    let index = 0;
    for (let row = 0; row < rowCount; row += 1) {
      const count = base + (row < extra ? 1 : 0);
      if (count <= 0) continue;
      rows.push(trees.slice(index, index + count));
      index += count;
    }
    return rows;
  };

  const measureRows = (rows: ForestTree[][]) => {
    let width = 0;
    let height = 0;
    rows.forEach((rowTrees, index) => {
      let rowWidth = 0;
      let rowHeight = 0;
      rowTrees.forEach((tree, treeIndex) => {
        if (treeIndex > 0) rowWidth += TOP_TREE_GAP;
        rowWidth += tree.width;
        rowHeight = Math.max(rowHeight, tree.height);
      });
      width = Math.max(width, rowWidth);
      if (index > 0) height += TOP_ROW_GAP;
      height += rowHeight;
    });
    return { width, height };
  };

  let rows = packBalanced(1);
  let bestScore = Number.POSITIVE_INFINITY;
  const rowLimit = Math.min(TOP_MAX_ROWS, Math.max(1, trees.length));
  for (let rowCount = 1; rowCount <= rowLimit; rowCount += 1) {
    const candidate = packBalanced(rowCount);
    const bounds = measureRows(candidate);
    const aspect = bounds.height > 0
      ? bounds.width / bounds.height
      : Number.POSITIVE_INFINITY;
    const score = Math.abs(aspect - TARGET_ASPECT);
    if (score < bestScore) {
      bestScore = score;
      rows = candidate;
    }
  }

  const positions = new Map<string, NodePosition>();
  let rowTop = LAYOUT_ORIGIN_Y;
  for (const rowTrees of rows) {
    if (rowTrees.length === 0) continue;
    let rowX = LAYOUT_ORIGIN_X;
    let rowHeight = 0;
    for (const tree of rowTrees) {
      for (const id of tree.nodes) {
        const position = forest.positions.get(id);
        if (!position) continue;
        positions.set(id, {
          x: Math.round(rowX + position.x - tree.minX),
          y: Math.round(rowTop + position.y),
        });
      }
      rowX += tree.width + TOP_TREE_GAP;
      rowHeight = Math.max(rowHeight, tree.height);
    }
    rowTop += rowHeight + TOP_ROW_GAP;
  }

  // 分组内部：按计划把直接子项递归展开到最终坐标。
  const result = new Map<string, NodePosition>();
  const expand = (nodeId: string) => {
    const position = positions.get(nodeId);
    const node = nodeById.get(nodeId);
    if (!position || !node) return;
    result.set(nodeId, position);
    if (node.kind !== "group") return;
    const plan = plans.get(nodeId);
    if (!plan) return;
    for (const [childId, relative] of plan.children) {
      const child = nodeById.get(childId);
      if (!child) continue;
      positions.set(childId, {
        x: position.x + relative.x,
        y: position.y + relative.y,
      });
      expand(childId);
    }
  };

  for (const unit of topLevel) expand(unit.id);
  return result;
}
