import type {
  NodePosition,
  NodeStatus,
  WorkflowEdge,
  WorkflowNode,
} from "@/api";

import { computeTopology } from "./graph";
import { buildDelayLayoutGraph, computeDelayPills } from "./delayPill";
import { groupModeOf, isCompactGroup } from "./shared";

/** 节点卡片尺寸估算（实际尺寸以 Vue Flow 量测为准，这里用于布局与空分组占位）。 */
export const NODE_WIDTH = 224;
export const NODE_HEIGHT = 68;

/** 分组容器内边距 / 标题栏高度。 */
// 内边距 / 组头高度都取 16 的倍数：分组包围盒与内部子节点才能同时落在点阵上。
export const GROUP_PADDING = 32;
export const GROUP_HEADER = 32;

/** 空分组（还没有子项）的默认尺寸。 */
export const DEFAULT_GROUP_WIDTH = 240;
export const DEFAULT_GROUP_HEIGHT = 120;

/** 分组内网格（简洁顺序 / 并行）：cell 间留出连线通道。 */
// 网格步长 = NODE_WIDTH + GRID_GAP_X / NODE_HEIGHT + GRID_GAP_Y，取 16 的倍数，
// 这样紧凑网格的每一行每一列都正好是整格，吸附不会把间距抹成 32/48 交替。
const GRID_GAP_X = 48;
const GRID_GAP_Y = 44;

/** 自动排列顶层的基础间距 / 原点。 */
export const LAYOUT_COLUMN_GAP = 80;
export const LAYOUT_ROW_GAP = 32;
export const LAYOUT_ORIGIN_X = 80;
export const LAYOUT_ORIGIN_Y = 64;

/** 画布网格步长，必须与 WorkflowCanvas 的 SNAP_GRID 一致。 */
export const LAYOUT_GRID = 16;

/** 把坐标吸附到画布网格。 */
export function snapToGrid(value: number): number {
  return Math.round(value / LAYOUT_GRID) * LAYOUT_GRID;
}

/** 思维导图：层间横向间距 / 兄弟子树纵向间距 / 多个根之间。 */
// 列间距要能容纳 88px 宽的延时药丸标签（居中后两侧仍有空隙）。
const MIND_COLUMN_GAP = 132;
const MIND_ROW_GAP = 32;
const MIND_ROOT_GAP = 56;

/** 长链换行：一行最多先按列数扫描，折行后的行间距（留出连线/标签通道）。 */
const WRAP_ROW_GAP = 72;

/** 单棵根树的目标宽高比；落在可接受区间内就不折行。 */
const TREE_TARGET_ASPECT = 1.9;
const TREE_NO_WRAP_MIN_ASPECT = 1.3;
const TREE_NO_WRAP_MAX_ASPECT = 2.6;
/** 层数太少的短链不折行（折了反而破坏父子从左到右的直觉）。 */
const MIN_WRAP_LEVELS = 4;

/**
 * 顶层多棵「根树」排布：
 * - 顶层单元（分组超级节点 / 未分组节点）先按思维导图拆成独立的根树；
 * - 根树再按「行 / 列」做二维打包：穷举连续分行方案（或按目标宽度分行），
 *   同时尝试按高度/面积的自由排序与 masonry 列打包，取空白最少且宽高比接近目标的方案；
 * - 树之间有连线时保持结构顺序（不重排），完全独立的树允许重排。
 */
const TOP_TREE_GAP = 72;
const TOP_ROW_GAP = 56;
const TOP_TARGET_ASPECT = 1.8;
/** 顶层最多尝试的行数（仅用于兜底排版，不再是硬性上限）。 */
const TOP_MAX_ROWS = 8;
/** 根树数量不超过该值时穷举全部分行方案；超过则用目标宽度贪心。 */
const TOP_EXHAUSTIVE_TREES = 14;

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

/**
 * 网格列数：按**像素宽高比**选优（卡片 224×68 是横向的，纯按数量 sqrt 会让容器很扁）。
 * 枚举 cols=1..n，rows=ceil(n/cols)，比较整组容器宽高 `cols*cellW+(cols-1)*gapX` 与
 * `rows*cellH+(rows-1)*gapY`，取 |log(width/height)| 最小（再用 |width-height| 破平局）。
 */
function chooseGridColumns(
  count: number,
  cellWidth: number,
  cellHeight: number,
): number {
  if (count <= 1) return 1;
  const baseWidth = GROUP_PADDING * 2;
  const baseHeight = GROUP_HEADER + GROUP_PADDING * 2;
  let bestColumns = 1;
  let bestScore = Number.POSITIVE_INFINITY;
  let bestDiff = Number.POSITIVE_INFINITY;
  for (let columns = 1; columns <= count; columns += 1) {
    const rows = Math.ceil(count / columns);
    const width =
      baseWidth + columns * cellWidth + Math.max(0, columns - 1) * GRID_GAP_X;
    const height =
      baseHeight + rows * cellHeight + Math.max(0, rows - 1) * GRID_GAP_Y;
    const ratio = width / Math.max(1, height);
    const score = Math.abs(Math.log(ratio));
    const diff = Math.abs(width - height);
    if (
      score < bestScore - 1e-9 ||
      (Math.abs(score - bestScore) <= 1e-9 && diff < bestDiff - 1e-9)
    ) {
      bestScore = score;
      bestDiff = diff;
      bestColumns = columns;
    }
  }
  return bestColumns;
}

/**
 * 分组内统一网格：
 * - `columnFirst = false`：按行填充（逐行从左到右，顶部对齐）——顺序简洁展示用；
 * - `columnFirst = true`：按列填充（每列从上到下，列从左到右，顶部对齐）——并行分组用，
 *   这样同一列的节点纵向连续，列末→下一列首不会绕大圈。
 * 列数由 `chooseGridColumns` 按像素宽高比选优，目标是分组容器接近 1:1。
 */
function planGrid(
  children: WorkflowNode[],
  sizeOf: SizeOf,
  columnFirst: boolean,
): GroupPlan {
  const count = children.length;
  if (count === 0) return emptyPlan();

  let cellWidth = NODE_WIDTH;
  let cellHeight = NODE_HEIGHT;
  for (const child of children) {
    const size = sizeOf(child);
    cellWidth = Math.max(cellWidth, size.width);
    cellHeight = Math.max(cellHeight, size.height);
  }

  const columns = chooseGridColumns(count, cellWidth, cellHeight);
  const rows = Math.ceil(count / columns);
  const positions = new Map<string, NodePosition>();
  children.forEach((child, index) => {
    const column = columnFirst
      ? Math.min(columns - 1, Math.floor(index / rows))
      : index % columns;
    const row = columnFirst ? index % rows : Math.floor(index / columns);
    positions.set(child.id, {
      x: GROUP_PADDING + column * (cellWidth + GRID_GAP_X),
      y: GROUP_HEADER + GROUP_PADDING + row * (cellHeight + GRID_GAP_Y),
    });
  });

  return {
    size: {
      width:
        GROUP_PADDING * 2 +
        columns * cellWidth +
        Math.max(0, columns - 1) * GRID_GAP_X,
      height:
        GROUP_HEADER +
        GROUP_PADDING * 2 +
        rows * cellHeight +
        Math.max(0, rows - 1) * GRID_GAP_Y,
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

  const buildChunksFromGroups = (groups: number[][]): WrapChunk[] => {
    const chunks: WrapChunk[] = [];
    for (const chunkLevels of groups) {
      if (chunkLevels.length === 0) continue;
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

  const buildChunks = (columnsPerRow: number): WrapChunk[] => {
    const groups: number[][] = [];
    for (let index = 0; index < levels.length; index += columnsPerRow) {
      groups.push(levels.slice(index, index + columnsPerRow));
    }
    return buildChunksFromGroups(groups);
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
  const candidates: WrapChunk[][] = [];
  // 等分折行：每行 N 列。
  for (let columns = levels.length; columns >= 1; columns -= 1) {
    candidates.push(buildChunks(columns));
  }
  // 两段切分：大块根树（如几个近似正方形的分组）等分未必最接近目标比例。
  for (let split = 1; split < levels.length; split += 1) {
    candidates.push(
      buildChunksFromGroups([levels.slice(0, split), levels.slice(split)]),
    );
  }

  for (const chunks of candidates) {
    const bounds = measure(chunks);
    const aspect = bounds.height > 0
      ? bounds.width / bounds.height
      : Number.POSITIVE_INFINITY;
    // 越接近目标比例越好；折行越多给一点点惩罚，避免无谓碎片化。
    const score =
      Math.abs(aspect - TREE_TARGET_ASPECT) + (chunks.length - 1) * 0.02;
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
  alignParent: "center" | "top" = "center",
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

    // 父节点默认垂直居中对齐子节点范围；「按连线」模式改为与第一个子节点顶部对齐，
    // 一对多分叉（如 FlClash → v2rayN / Clash Verge）保持水平对齐更直观。
    let y: number;
    if (alignParent === "top") {
      const firstKid = positions.get(kids[0]);
      y = firstKid ? firstKid.y : top;
    } else {
      const center = (firstChildTop + lastChildBottom) / 2;
      y = center - unitHeight / 2;
    }
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
  alignParentTop: boolean,
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

  const forest = layoutUnitForest(
    units,
    pairs,
    sizeOf,
    compare,
    alignParentTop ? "top" : "center",
  );
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
  // 折叠成药丸的延时节点不渲染，也不占分组网格槽位。
  const pills = computeDelayPills(nodes, edges);
  const visibleNodes =
    pills.size === 0 ? nodes : nodes.filter((node) => !pills.has(node.id));
  const { childrenOf } = buildGroupChildren(visibleNodes);
  const topology = computeTopology(visibleNodes, edges);
  const stepOf = topology.stepOf;
  const draftIndex = new Map(visibleNodes.map((node, index) => [node.id, index]));
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
    const rawChildren = childrenOf.get(group.id) ?? [];
    const mode = groupModeOf(group);
    let plan: GroupPlan;
    if (rawChildren.length === 0) {
      plan = emptyPlan();
    } else if (mode === "parallel") {
      // 并行：列优先网格（每列自上而下、列从左到右）。
      plan = planGrid(rawChildren, sizeOf, true);
    } else if (mode === "sequence" && isCompactGroup(group)) {
      // 顺序 + 简洁展示：按 workflow.nodes 顺序的行优先 Wrap 网格。
      plan = planGrid(rawChildren, sizeOf, false);
    } else {
      // 按连线（graph）/ 顺序但不简洁：按子图拓扑做思维导图布局。
      const children = [...rawChildren].sort(compare);
      plan = planSequentialGroup(
        children,
        edges,
        childrenOf,
        sizeOf,
        compare,
        mode === "graph",
      );
    }
    resolving.delete(group.id);
    plans.set(group.id, plan);
    return plan;
  }

  for (const node of visibleNodes) {
    if (node.kind === "group") resolve(node);
  }
  return plans;
}

// ---------------------------------------------------------------------------
// 顶层根树打包
//
// 每棵根树是一个矩形的「超级单元」。旧实现只按「行数均分」切行，行内顶部对齐，
// 行高取该行最高树，导致矮树下方留下大块空白、分组/树之间出现长距离空隙。
// 这里改成二维打包搜索：
// - 行打包：穷举「连续分段」方案（树数量大时按目标宽度贪心 + 均分行数兜底），
//   同时尝试按高度/宽度/面积排序（仅当树之间没有连线时允许重排）；
// - 列打包：同样的连续分段，竖排成列（适合一批扁宽树）；
// - masonry：按固定行数/列数「填最短的一行/列」，进一步压掉行高不齐的空白。
// 评分 = 宽高比偏差 + 空白率（未被树覆盖的 bbox 面积）+ 轻微行/列数惩罚。
// ---------------------------------------------------------------------------

interface TreePlacement {
  tree: ForestTree;
  x: number;
  y: number;
}

interface PackedForest {
  placements: TreePlacement[];
  width: number;
  height: number;
  rows: number;
  columns: number;
  whitespace: number;
  aspect: number;
}

/** 树的面积（按自身 bbox），用于空白率估算。 */
function treeArea(tree: ForestTree): number {
  return Math.max(1, tree.width) * Math.max(1, tree.height);
}

function measurePlacements(
  placements: TreePlacement[],
  rows: number,
  columns: number,
): PackedForest {
  let width = 0;
  let height = 0;
  let covered = 0;
  for (const placement of placements) {
    width = Math.max(width, placement.x + placement.tree.width);
    height = Math.max(height, placement.y + placement.tree.height);
    covered += treeArea(placement.tree);
  }
  const area = Math.max(1, width) * Math.max(1, height);
  return {
    placements,
    width,
    height,
    rows,
    columns,
    whitespace: Math.max(0, 1 - covered / area),
    aspect: width / Math.max(1, height),
  };
}

/** 按行放置：breaks 是每行包含的树数量（顺序连续）。 */
function placeRows(order: ForestTree[], breaks: number[]): PackedForest {
  const placements: TreePlacement[] = [];
  let cursor = 0;
  let y = 0;
  let rowCount = 0;
  for (const count of breaks) {
    if (count <= 0) continue;
    const rowTrees = order.slice(cursor, cursor + count);
    if (rowTrees.length === 0) continue;
    cursor += count;
    rowCount += 1;
    let x = 0;
    let rowHeight = 0;
    for (const tree of rowTrees) {
      placements.push({ tree, x, y });
      x += tree.width + TOP_TREE_GAP;
      rowHeight = Math.max(rowHeight, tree.height);
    }
    y += rowHeight + TOP_ROW_GAP;
  }
  return measurePlacements(placements, rowCount, 0);
}

/** 按列放置：breaks 是每列包含的树数量（顺序连续）。 */
function placeColumns(order: ForestTree[], breaks: number[]): PackedForest {
  const placements: TreePlacement[] = [];
  let cursor = 0;
  let x = 0;
  let columnCount = 0;
  for (const count of breaks) {
    if (count <= 0) continue;
    const columnTrees = order.slice(cursor, cursor + count);
    if (columnTrees.length === 0) continue;
    cursor += count;
    columnCount += 1;
    let y = 0;
    let columnWidth = 0;
    for (const tree of columnTrees) {
      placements.push({ tree, x, y });
      y += tree.height + TOP_ROW_GAP;
      columnWidth = Math.max(columnWidth, tree.width);
    }
    x += columnWidth + TOP_TREE_GAP;
  }
  return measurePlacements(placements, 0, columnCount);
}

/** masonry 行：固定行数，依次把树放进当前累计宽度最短的一行。 */
function placeMasonryRows(order: ForestTree[], rowCount: number): PackedForest {
  const count = Math.max(1, Math.min(rowCount, order.length));
  const rows: TreePlacement[][] = Array.from({ length: count }, () => []);
  const widths = new Array<number>(count).fill(0);
  for (const tree of order) {
    let target = 0;
    for (let index = 1; index < count; index += 1) {
      if (widths[index] < widths[target] - 1e-6) target = index;
    }
    widths[target] += tree.width + TOP_TREE_GAP;
    rows[target].push({ tree, x: 0, y: 0 });
  }
  const placements: TreePlacement[] = [];
  let y = 0;
  for (const row of rows) {
    let x = 0;
    let rowHeight = 0;
    for (const item of row) {
      placements.push({ tree: item.tree, x, y });
      x += item.tree.width + TOP_TREE_GAP;
      rowHeight = Math.max(rowHeight, item.tree.height);
    }
    y += rowHeight + TOP_ROW_GAP;
    widths.push(x);
  }
  return measurePlacements(placements, count, 0);
}

/** masonry 列：固定列数，依次把树放进当前累计高度最短的一列。 */
function placeMasonryColumns(order: ForestTree[], columnCount: number): PackedForest {
  const count = Math.max(1, Math.min(columnCount, order.length));
  const columns: TreePlacement[][] = Array.from({ length: count }, () => []);
  const heights = new Array<number>(count).fill(0);
  for (const tree of order) {
    let target = 0;
    for (let index = 1; index < count; index += 1) {
      if (heights[index] < heights[target] - 1e-6) target = index;
    }
    heights[target] += tree.height + TOP_ROW_GAP;
    columns[target].push({ tree, x: 0, y: 0 });
  }
  const placements: TreePlacement[] = [];
  let x = 0;
  for (const column of columns) {
    let y = 0;
    let columnWidth = 0;
    for (const item of column) {
      placements.push({ tree: item.tree, x, y });
      y += item.tree.height + TOP_ROW_GAP;
      columnWidth = Math.max(columnWidth, item.tree.width);
    }
    x += columnWidth + TOP_TREE_GAP;
  }
  return measurePlacements(placements, 0, count);
}

/** 枚举把 count 个元素切成若干连续段的全部方案（段数不定）。 */
function* allPartitions(count: number): Generator<number[]> {
  if (count <= 0) {
    yield [];
    return;
  }
  const buffer: number[] = [];
  function* walk(remaining: number): Generator<number[]> {
    if (remaining === 0) {
      yield [...buffer];
      return;
    }
    for (let take = 1; take <= remaining; take += 1) {
      buffer.push(take);
      yield* walk(remaining - take);
      buffer.pop();
    }
  }
  yield* walk(count);
}

function partitionScore(packed: PackedForest): number {
  const aspect = Math.max(0.05, packed.aspect);
  const aspectPenalty = Math.abs(Math.log(aspect / TOP_TARGET_ASPECT));
  // 极端宽/高再额外惩罚：避免 4:1 或 0.6:1 这类难看的图。
  const extreme =
    aspect > 3.4 ? (aspect - 3.4) * 0.6 : aspect < 0.85 ? (0.85 - aspect) * 0.9 : 0;
  const rowsPenalty = Math.max(0, packed.rows - 1) * 0.015;
  const columnsPenalty = Math.max(0, packed.columns - 1) * 0.01;
  // 空白权重高于宽高比：用户反馈「分组/树之间大块空白」比「必须 16:9」更刺眼，
  // 只要整体仍在 1.4–2.4 的 landscape 区间内即可。
  return aspectPenalty * 1.3 + packed.whitespace * 3.4 + extreme + rowsPenalty + columnsPenalty;
}

/** 大的树先排，让行/列更容易均衡；仅在树之间无连线时允许。 */
function sortTrees(trees: ForestTree[], by: "height" | "width" | "area"): ForestTree[] {
  const value = (tree: ForestTree) =>
    by === "height" ? tree.height : by === "width" ? tree.width : treeArea(tree);
  return [...trees].sort(
    (left, right) =>
      value(right) - value(left) || left.rootId.localeCompare(right.rootId),
  );
}

/**
 * 顶层根树打包：返回每棵树相对左上角的坐标。
 * `hasCrossTreeEdges = true` 时保持原始结构顺序（不重排，保证依赖方向可读）。
 */
function packForestTrees(
  trees: ForestTree[],
  hasCrossTreeEdges: boolean,
): PackedForest {
  if (trees.length === 0) {
    return {
      placements: [],
      width: 0,
      height: 0,
      rows: 0,
      columns: 0,
      whitespace: 0,
      aspect: 1,
    };
  }

  const orders: ForestTree[][] = [trees];
  if (!hasCrossTreeEdges && trees.length > 1) {
    orders.push(sortTrees(trees, "height"));
    orders.push(sortTrees(trees, "area"));
    orders.push(sortTrees(trees, "width"));
  }

  let best: PackedForest | null = null;
  const consider = (candidate: PackedForest) => {
    if (candidate.placements.length === 0) return;
    if (!best || partitionScore(candidate) < partitionScore(best) - 1e-9) {
      best = candidate;
    }
  };

  for (const order of orders) {
    if (order.length <= TOP_EXHAUSTIVE_TREES) {
      for (const breaks of allPartitions(order.length)) {
        consider(placeRows(order, breaks));
        consider(placeColumns(order, breaks));
      }
    } else {
      // 树太多时：均分行数 + 按目标宽度贪心切分。
      const totalArea = order.reduce((sum, tree) => sum + treeArea(tree), 0);
      const baseWidth = Math.sqrt(totalArea * TOP_TARGET_ASPECT);
      const targets = [0.55, 0.7, 0.85, 1, 1.15, 1.3, 1.5, 1.8].map(
        (scale) => baseWidth * scale,
      );
      for (let rowCount = 1; rowCount <= Math.min(TOP_MAX_ROWS, order.length); rowCount += 1) {
        const base = Math.floor(order.length / rowCount);
        const extra = order.length % rowCount;
        const breaks: number[] = [];
        for (let row = 0; row < rowCount; row += 1) {
          breaks.push(base + (row < extra ? 1 : 0));
        }
        consider(placeRows(order, breaks));
      }
      for (const target of targets) {
        const breaks: number[] = [];
        let current = 0;
        let width = 0;
        for (const tree of order) {
          if (current > 0 && width + tree.width + TOP_TREE_GAP > target) {
            breaks.push(current);
            current = 0;
            width = 0;
          }
          current += 1;
          width += tree.width + TOP_TREE_GAP;
        }
        if (current > 0) breaks.push(current);
        consider(placeRows(order, breaks));
        consider(placeColumns(order, breaks));
      }
    }

    // masonry：固定行/列数，填最空的一行/列。
    for (let count = 1; count <= Math.min(TOP_MAX_ROWS, order.length); count += 1) {
      consider(placeMasonryRows(order, count));
      consider(placeMasonryColumns(order, count));
    }
  }

  // 兜底：至少按「每棵树一行」放置（正常搜索一定能产出更优解）。
  return best ?? placeRows(trees, trees.map(() => 1));
}

/** 只取尺寸（画布需要根据分组计划更新容器外框）。 */
export function computeGroupSizes(  nodes: WorkflowNode[],
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

  // 顶层根树是否存在「跨树连线」：没有连线时允许重排/列打包（纯二维装箱），
  // 有连线时保持结构顺序，保证依赖方向可读。
  const treeOfNode = new Map<string, number>();
  forest.trees.forEach((tree, index) => {
    for (const id of tree.nodes) treeOfNode.set(id, index);
  });
  let hasCrossTreeEdges = false;
  for (const [source, target] of pairs) {
    if (treeOfNode.get(source) !== treeOfNode.get(target)) {
      hasCrossTreeEdges = true;
      break;
    }
  }
  const packed = packForestTrees(forest.trees, hasCrossTreeEdges);

  const positions = new Map<string, NodePosition>();
  for (const placement of packed.placements) {
    const tree = placement.tree;
    for (const id of tree.nodes) {
      const position = forest.positions.get(id);
      if (!position) continue;
      positions.set(id, {
        x: Math.round(LAYOUT_ORIGIN_X + placement.x + position.x - tree.minX),
        y: Math.round(LAYOUT_ORIGIN_Y + placement.y + position.y),
      });
    }
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

  // 吸附到画布网格（与 WorkflowCanvas 的 SNAP_GRID 一致）：布局内部的常量
  // （原点 60、内边距 24、行距 40 …）都不是 16 的倍数，若不吸附，自动排列出来的
  // 节点会落在点阵网格之间，和手动拖动（按 16px 吸附）的观感不一致。
  for (const [id, position] of result) {
    const snapped = {
      x: snapToGrid(position.x),
      y: snapToGrid(position.y),
    };
    if (snapped.x !== position.x || snapped.y !== position.y) {
      result.set(id, snapped);
    }
  }
  return result;
}
