/**
 * 纯函数边路由：
 * - 输入：可见节点（flow 坐标 + 量测宽高，可含分组容器）、边（source/target、是否带延时标签）；
 * - 输出：每条边的正交折线 path（圆角）+ 标签坐标（供 EdgeLabelRenderer 药丸定位）。
 *
 * 策略概要：
 * 1. 端点端口：普通卡片取左/右/上/下边中点；分组容器取「组边缘锚点」
 *    （左/右边缘在组头高度中线，上/下边缘在水平中线），支持 group→group 边；
 * 2. 障碍：所有可见节点矩形（含分组容器，避免穿过组体/组头）。每条边会忽略
 *    自己的两端节点、以及「包含端点的祖先分组容器」的组体；但祖先的组头条带
 *    仍然是障碍，避免从组头穿出；
 * 3. 全局 lane：按整图列间/行间空隙生成竖直/水平通道槽位；同一条通道里的多条边
 *    按源/目标坐标排序后分到不同槽位（确定性、无随机），减少同向重叠；
 * 4. 候选路径：直连、H-V-H、V-H-V、L 形、行间/列间通道绕行、整图外侧绕行；
 *    评分 = 碰撞数 + 标签碰撞 + 拐点数 + 路径长度 + 方向偏好；
 * 5. 标签始终落在路径线段上（可沿线段滑动避让），同时避开所有障碍矩形；
 *    同一区域的多个标签沿线段方向错开，药丸不会压住卡片或组头。
 */
import { GROUP_HEADER } from "./layout";

export interface RoutingNode {
  id: string;
  x: number;
  y: number;
  width: number;
  height: number;
  /**
   * 可选：该节点是分组容器（含组头）。不传时按「矩形包含其它节点」启发式识别，
   * 因此旧调用方无需改动也能正确避让分组容器。
   */
  isGroup?: boolean;
  /** 可选：组头高度（默认与 layout.ts 的 GROUP_HEADER 一致）。 */
  headerHeight?: number;
}

export interface RoutingEdgeInput {
  id: string;
  sourceId: string;
  targetId: string;
  /** 是否为折叠了延时药丸的合成边（标签是 88×28 的药丸）。 */
  hasLabel?: boolean;
  /** 顺序展示边：允许目标在下方但不同列时也走「小黄点」斜线（更短）。 */
  verticalCorner?: boolean;
}

export interface EdgeRoute {
  /** SVG path（正交折线 + 圆角，flow 坐标）。 */
  path: string;
  labelX: number;
  labelY: number;
  /** 终点与最后一段方向（供画布顶层箭头叠加层使用）。 */
  endX?: number;
  endY?: number;
  endAngle?: number;
  /** 纵向排列的特殊斜线：端点画在卡片角上（源右下 / 目标左上）。 */
  cornerStart?: Point;
  cornerEnd?: Point;
}

export interface RoutingOptions {
  /** 端点出线短桩长度 */
  stub?: number;
  /** 节点障碍外扩 */
  padding?: number;
  /** 圆角半径 */
  radius?: number;
  /** 同一通道内的 lane 间距 */
  laneGap?: number;
}

export type Point = [number, number];

type Side = "left" | "right" | "top" | "bottom";

interface Obstacle {
  id: string;
  left: number;
  top: number;
  right: number;
  bottom: number;
  isGroup: boolean;
  /** 分组容器的组头高度（仅 isGroup 时有意义）。 */
  headerHeight?: number;
}

interface Port {
  x: number;
  y: number;
  side: Side;
}

interface PortPair {
  source: Port;
  target: Port;
  /** 相对主方向的额外代价（主方向 0，次方向更大）。 */
  penalty: number;
}

interface LaneGap {
  key: string;
  /** 槽位坐标（沿垂直方向是 x，水平方向是 y），升序。 */
  slots: number[];
  min: number;
  max: number;
}

interface RouteCandidate {
  points: Point[];
  penalty: number;
  corners?: { start: Point; end: Point };
  /** 用于标识通道类型（影响同名通道的排序/稳定性）。 */
  kind: string;
}

interface LabelSpot {
  x: number;
  y: number;
  axis: "vertical" | "horizontal";
  min: number;
  max: number;
  collisions: number;
}

const EPSILON = 1.2;
/** 组头高度的兜底（与 layout.ts 的 GROUP_HEADER 一致）。 */
const FALLBACK_GROUP_HEADER = GROUP_HEADER || 30;
const DEFAULT_LABEL_WIDTH = 56;
const DEFAULT_LABEL_HEIGHT = 20;
const PILL_WIDTH = 88;
const PILL_HEIGHT = 28;

function nodeCenterY(node: RoutingNode): number {
  return node.y + node.height / 2;
}

function dedupePoints(points: Point[]): Point[] {
  const result: Point[] = [];
  for (const point of points) {
    const prev = result[result.length - 1];
    if (
      prev &&
      Math.abs(prev[0] - point[0]) < 0.5 &&
      Math.abs(prev[1] - point[1]) < 0.5
    ) {
      continue;
    }
    result.push(point);
  }
  return result;
}

function simplifyPoints(points: Point[]): Point[] {
  const deduped = dedupePoints(points);
  const result: Point[] = [];
  for (const point of deduped) {
    const length = result.length;
    if (length >= 2) {
      const a = result[length - 2];
      const b = result[length - 1];
      const collinear =
        (Math.abs(a[0] - b[0]) < 0.5 && Math.abs(b[0] - point[0]) < 0.5) ||
        (Math.abs(a[1] - b[1]) < 0.5 && Math.abs(b[1] - point[1]) < 0.5);
      if (collinear) {
        result[length - 1] = point;
        continue;
      }
    }
    result.push(point);
  }
  return result;
}

/** 把折线转成带圆角的 SVG path。 */
function buildRoundedPath(rawPoints: Point[], radius: number): string {
  const points = simplifyPoints(rawPoints);
  if (points.length === 0) return "";
  const round = (value: number) => Math.round(value * 10) / 10;
  let path = `M ${round(points[0][0])} ${round(points[0][1])}`;
  for (let index = 1; index < points.length - 1; index += 1) {
    const prev = points[index - 1];
    const current = points[index];
    const next = points[index + 1];
    const inLength = Math.hypot(current[0] - prev[0], current[1] - prev[1]);
    const outLength = Math.hypot(next[0] - current[0], next[1] - current[1]);
    const r = Math.min(radius, inLength / 2, outLength / 2);
    if (r < 0.5) {
      path += ` L ${round(current[0])} ${round(current[1])}`;
      continue;
    }
    const beforeRatio = r / (inLength || 1);
    const afterRatio = r / (outLength || 1);
    const beforeX = current[0] + (prev[0] - current[0]) * beforeRatio;
    const beforeY = current[1] + (prev[1] - current[1]) * beforeRatio;
    const afterX = current[0] + (next[0] - current[0]) * afterRatio;
    const afterY = current[1] + (next[1] - current[1]) * afterRatio;
    path += ` L ${round(beforeX)} ${round(beforeY)}`;
    path += ` Q ${round(current[0])} ${round(current[1])} ${round(afterX)} ${round(afterY)}`;
  }
  const last = points[points.length - 1];
  path += ` L ${round(last[0])} ${round(last[1])}`;
  return path;
}

/** 合并区间（含容差），用于从节点包围盒之间的空隙生成通道。 */
function mergeIntervals(intervals: Array<[number, number]>, tolerance = 8): Array<[number, number]> {
  const sorted = [...intervals].sort((left, right) => left[0] - right[0]);
  const merged: Array<[number, number]> = [];
  for (const interval of sorted) {
    const last = merged[merged.length - 1];
    if (last && interval[0] <= last[1] + tolerance) {
      last[1] = Math.max(last[1], interval[1]);
    } else {
      merged.push([interval[0], interval[1]]);
    }
  }
  return merged;
}

/**
 * 由整图列/行空隙生成通道槽位。
 * 每个空隙按宽度切出 1–5 个槽位（laneGap 间距、居中），让同一通道里的多条边
 * 能分到不同 lane，而不是全部挤在中线。
 */
function buildLaneGaps(
  intervals: Array<[number, number]>,
  laneGap: number,
  axisName: string,
): LaneGap[] {
  if (intervals.length === 0) return [];
  const merged = mergeIntervals(intervals);
  const gaps: LaneGap[] = [];
  for (let index = 1; index < merged.length; index += 1) {
    const start = merged[index - 1][1];
    const end = merged[index][0];
    const width = end - start;
    if (width < 24) continue;
    const count = Math.max(1, Math.min(5, Math.floor((width - 8) / laneGap)));
    const center = (start + end) / 2;
    const slots: number[] = [];
    for (let slot = 0; slot < count; slot += 1) {
      slots.push(Math.round((center + (slot - (count - 1) / 2) * laneGap) * 10) / 10);
    }
    gaps.push({ key: `${axisName}:${index}`, slots, min: start, max: end });
  }
  return gaps;
}

function nearestGap(
  gaps: LaneGap[],
  target: number,
  rangeMin: number,
  rangeMax: number,
): { gap: LaneGap; index: number } | null {  // 优先取中心落在 [rangeMin, rangeMax] 内的空隙，其次取最近空隙。
  let inside: { gap: LaneGap; index: number; distance: number } | null = null;
  let outside: { gap: LaneGap; index: number; distance: number } | null = null;
  const min = Math.min(rangeMin, rangeMax);
  const max = Math.max(rangeMin, rangeMax);
  gaps.forEach((gap, index) => {
    const center = (gap.min + gap.max) / 2;
    const distance = Math.abs(center - target);
    if (center >= min && center <= max) {
      if (!inside || distance < inside.distance) inside = { gap, index, distance };
    } else if (!outside || distance < outside.distance) {
      outside = { gap, index, distance };
    }
  });
  return inside ?? outside;
}

/** 从空隙里按顺序取槽位（同通道的第 n 条边用第 n 个槽位，超出后向外叠）。 */
function pickSlot(gap: LaneGap, order: number, laneGap: number): number {
  const count = gap.slots.length;
  const round = Math.floor(order / count);
  const slot = gap.slots[order % count];
  if (round === 0) return slot;
  const offset = round * laneGap;
  const sign = round % 2 === 1 ? 1 : -1;
  const candidate = slot + sign * offset;
  if (candidate <= gap.min + 6) return Math.min(gap.max - 6, gap.min + 6 + offset);
  if (candidate >= gap.max - 6) return Math.max(gap.min + 6, gap.max - 6 - offset);
  return candidate;
}

/**
 * 候选 lane 裁剪：离行进区间太远的 lane 最终也会被夹回区间，没必要参与评分；
 * 去重后按离区间中点由近到远保留前 N 个，控制候选数量（性能关键）。
 */
function narrowLanes(
  lanes: number[],
  rangeMin: number,
  rangeMax: number,
  limit = 10,
): number[] {
  const center = (rangeMin + rangeMax) / 2;
  const unique = new Map<number, number>();
  for (const raw of lanes) {
    const key = Math.round(raw * 10) / 10;
    if (!unique.has(key)) unique.set(key, raw);
  }
  return [...unique.values()]
    .filter((lane) => lane >= rangeMin - 120 && lane <= rangeMax + 120)
    .sort((left, right) => Math.abs(left - center) - Math.abs(right - center))
    .slice(0, limit);
}

/**
 * 局部通道：只考虑「与这条边的行进区间相交」的障碍，重新切出空隙。
 * 全局空隙会被跨行的大矩形（例如空分组容器）撑没，局部空隙能保证
 * 组内连线在组内绕开单个挡路卡片，而不是绕到整图外侧。
 */
function collectLocalLanes(
  obstacles: Obstacle[],
  bandMin: number,
  bandMax: number,
  axis: "x" | "y",
  laneGap: number,
): number[] {
  const intervals: Array<[number, number]> = [];
  for (const rect of obstacles) {
    const lo = axis === "x" ? rect.top : rect.left;
    const hi = axis === "x" ? rect.bottom : rect.right;
    if (hi < bandMin - 4 || lo > bandMax + 4) continue;
    intervals.push(axis === "x" ? [rect.left, rect.right] : [rect.top, rect.bottom]);
  }
  if (intervals.length === 0) return [];
  const merged = mergeIntervals(intervals, 4);
  const lanes: number[] = [];
  const first = merged[0];
  const last = merged[merged.length - 1];
  lanes.push(first[0] - 12, first[0] - 30, last[1] + 12, last[1] + 30);
  for (const [start, end] of merged) {
    lanes.push(start - 12, end + 12);
  }
  for (let index = 1; index < merged.length; index += 1) {
    const start = merged[index - 1][1];
    const end = merged[index][0];
    const width = end - start;
    if (width < 20) continue;
    const center = (start + end) / 2;
    lanes.push(center, center - laneGap, center + laneGap);
  }
  return lanes;
}

/** 线段（水平、垂直或斜线）是否穿过障碍矩形（留 EPSILON 容差）。 */
function segmentHitsRect(a: Point, b: Point, rect: Obstacle): boolean {
  if (Math.abs(a[1] - b[1]) < 0.5) {
    const y = a[1];
    const minX = Math.min(a[0], b[0]);
    const maxX = Math.max(a[0], b[0]);
    return (
      y > rect.top + EPSILON &&
      y < rect.bottom - EPSILON &&
      maxX > rect.left + EPSILON &&
      minX < rect.right - EPSILON
    );
  }
  if (Math.abs(a[0] - b[0]) < 0.5) {
    const x = a[0];
    const minY = Math.min(a[1], b[1]);
    const maxY = Math.max(a[1], b[1]);
    return (
      x > rect.left + EPSILON &&
      x < rect.right - EPSILON &&
      maxY > rect.top + EPSILON &&
      minY < rect.bottom - EPSILON
    );
  }
  // 斜线：Liang-Barsky 裁剪，判断线段是否与矩形内部相交。
  const left = rect.left + EPSILON;
  const right = rect.right - EPSILON;
  const top = rect.top + EPSILON;
  const bottom = rect.bottom - EPSILON;
  if (left >= right || top >= bottom) return false;
  const dx = b[0] - a[0];
  const dy = b[1] - a[1];
  let t0 = 0;
  let t1 = 1;
  const clip = (p: number, q: number): boolean => {
    if (Math.abs(p) < 1e-9) return q >= 0;
    const r = q / p;
    if (p < 0) {
      if (r > t1) return false;
      if (r > t0) t0 = r;
    } else {
      if (r < t0) return false;
      if (r < t1) t1 = r;
    }
    return true;
  };
  return (
    clip(-dx, a[0] - left) &&
    clip(dx, right - a[0]) &&
    clip(-dy, a[1] - top) &&
    clip(dy, bottom - a[1]) &&
    t0 < t1
  );
}

function countCollisions(
  points: Point[],
  obstacles: Obstacle[],
  cap = 3,
): number {
  let collisions = 0;
  for (let index = 1; index < points.length; index += 1) {
    const a = points[index - 1];
    const b = points[index];
    if (Math.abs(a[0] - b[0]) < 0.5 && Math.abs(a[1] - b[1]) < 0.5) continue;
    const minX = Math.min(a[0], b[0]);
    const maxX = Math.max(a[0], b[0]);
    const minY = Math.min(a[1], b[1]);
    const maxY = Math.max(a[1], b[1]);
    for (const rect of obstacles) {
      if (
        maxX < rect.left - 1 ||
        minX > rect.right + 1 ||
        maxY < rect.top - 1 ||
        minY > rect.bottom + 1
      ) {
        continue;
      }
      if (segmentHitsRect(a, b, rect)) {
        collisions += 1;
        if (collisions >= cap) return collisions;
      }
    }
  }
  return collisions;
}

/** 障碍物网格索引：碰撞检测只查线段包围盒覆盖的格子，边多时快 3–10 倍。 */
interface ObstacleIndex {
  cell: number;
  buckets: Map<string, Obstacle[]>;
}

const OBSTACLE_CELL = 240;

function buildObstacleIndex(obstacles: Obstacle[]): ObstacleIndex {
  const buckets = new Map<string, Obstacle[]>();
  for (const rect of obstacles) {
    const startX = Math.floor(rect.left / OBSTACLE_CELL);
    const endX = Math.floor(rect.right / OBSTACLE_CELL);
    const startY = Math.floor(rect.top / OBSTACLE_CELL);
    const endY = Math.floor(rect.bottom / OBSTACLE_CELL);
    for (let cellX = startX; cellX <= endX; cellX += 1) {
      for (let cellY = startY; cellY <= endY; cellY += 1) {
        const key = `${cellX}:${cellY}`;
        const bucket = buckets.get(key);
        if (bucket) bucket.push(rect);
        else buckets.set(key, [rect]);
      }
    }
  }
  return { cell: OBSTACLE_CELL, buckets };
}

function countCollisionsIndexed(
  points: Point[],
  index: ObstacleIndex,
  cap = 3,
): number {
  let collisions = 0;
  for (let pointIndex = 1; pointIndex < points.length; pointIndex += 1) {
    const a = points[pointIndex - 1];
    const b = points[pointIndex];
    if (Math.abs(a[0] - b[0]) < 0.5 && Math.abs(a[1] - b[1]) < 0.5) continue;
    const minX = Math.min(a[0], b[0]);
    const maxX = Math.max(a[0], b[0]);
    const minY = Math.min(a[1], b[1]);
    const maxY = Math.max(a[1], b[1]);
    const startX = Math.floor((minX - 1) / index.cell);
    const endX = Math.floor((maxX + 1) / index.cell);
    const startY = Math.floor((minY - 1) / index.cell);
    const endY = Math.floor((maxY + 1) / index.cell);
    const visited = new Set<Obstacle>();
    for (let cellX = startX; cellX <= endX; cellX += 1) {
      for (let cellY = startY; cellY <= endY; cellY += 1) {
        const bucket = index.buckets.get(`${cellX}:${cellY}`);
        if (!bucket) continue;
        for (const rect of bucket) {
          if (visited.has(rect)) continue;
          visited.add(rect);
          if (segmentHitsRect(a, b, rect)) {
            collisions += 1;
            if (collisions >= cap) return collisions;
          }
        }
      }
    }
  }
  return collisions;
}

function pathBends(points: Point[]): number {
  return Math.max(0, simplifyPoints(points).length - 2);
}

function pathLength(points: Point[]): number {
  let length = 0;
  for (let index = 1; index < points.length; index += 1) {
    length +=
      Math.abs(points[index][0] - points[index - 1][0]) +
      Math.abs(points[index][1] - points[index - 1][1]);
  }
  return length;
}

function rectContains(outer: Obstacle, inner: { x: number; y: number; width: number; height: number }): boolean {
  const eps = 2;
  return (
    outer.left - eps <= inner.x &&
    outer.top - eps <= inner.y &&
    outer.right + eps >= inner.x + inner.width &&
    outer.bottom + eps >= inner.y + inner.height
  );
}

/** 端点端口：分组容器用「组头中线」的边缘锚点，普通卡片用边中点。 */
function makePort(node: Obstacle, side: Side, offset = 0): Port {
  const height = node.bottom - node.top;
  const header = node.isGroup
    ? Math.min(height / 2, node.headerHeight ?? FALLBACK_GROUP_HEADER)
    : 0;
  const centerY = node.top + (header > 0 ? header / 2 : height / 2);
  const centerX = (node.left + node.right) / 2;
  // 同一条边上的多条连线错开锚点，避免两段线在卡片边缘完全重合。
  const clampOffset = (base: number, min: number, max: number) =>
    Math.min(max, Math.max(min, base + offset));
  switch (side) {
    case "left":
      return { x: node.left, y: clampOffset(centerY, node.top + 8, node.bottom - 8), side };
    case "right":
      return { x: node.right, y: clampOffset(centerY, node.top + 8, node.bottom - 8), side };
    case "top":
      return { x: clampOffset(centerX, node.left + 12, node.right - 12), y: node.top, side };
    case "bottom":
      return { x: clampOffset(centerX, node.left + 12, node.right - 12), y: node.bottom, side };
    default:
      return { x: centerX, y: centerY, side };
  }
}

/** 根据两个矩形的位置关系挑选主/次端口对。 */
function buildPortPairs(
  source: Obstacle,
  target: Obstacle,
  offsets: { source?: number; target?: number } = {},
): PortPair[] {
  const sourceCenterX = (source.left + source.right) / 2;
  const sourceCenterY = (source.top + source.bottom) / 2;
  const targetCenterX = (target.left + target.right) / 2;
  const targetCenterY = (target.top + target.bottom) / 2;

  const horizontalGap = Math.max(
    source.left - target.right,
    target.left - source.right,
  );
  const verticalGap = Math.max(
    source.top - target.bottom,
    target.top - source.bottom,
  );
  const targetRight = targetCenterX > sourceCenterX;
  const targetBelow = targetCenterY > sourceCenterY;

  const pairs: PortPair[] = [];
  const push = (sourceSide: Side, targetSide: Side, penalty: number) => {
    pairs.push({
      source: makePort(source, sourceSide, offsets.source ?? 0),
      target: makePort(target, targetSide, offsets.target ?? 0),
      penalty,
    });
  };

  const horizontalPossible =
    horizontalGap >= 8 || (verticalGap < 8 && Math.abs(targetCenterX - sourceCenterX) >= 0);
  const verticalPossible = verticalGap >= 8 || !horizontalPossible;

  if (horizontalPossible) {
    if (targetRight) push("right", "left", 0);
    else push("left", "right", 0);
  }
  if (verticalPossible) {
    if (targetBelow) push("bottom", "top", horizontalPossible ? 26 : 0);
    else push("top", "bottom", horizontalPossible ? 26 : 0);
  }
  if (pairs.length === 0) {
    push(targetRight ? "right" : "left", targetRight ? "left" : "right", 0);
  }
  return pairs;
}

/** 水平端口对的 H-V-H 候选：竖直段落在全局 lane 上。 */
function buildHorizontalCandidates(
  pair: PortPair,
  laneXs: number[],
  stub: number,
): RouteCandidate[] {
  const sp = pair.source;
  const tp = pair.target;
  const sourceDirection = sp.side === "right" ? 1 : -1;
  const targetDirection = tp.side === "right" ? 1 : -1;
  const sx = sp.x + sourceDirection * stub;
  const tx = tp.x + targetDirection * stub;
  const minLane = Math.min(sx, tx);
  const maxLane = Math.max(sx, tx);
  const candidates: RouteCandidate[] = [];

  if (Math.abs(sp.y - tp.y) < 1) {
    candidates.push({
      points: [
        [sp.x, sp.y],
        [tp.x, tp.y],
      ],
      penalty: 4,
      kind: "straight",
    });
  } else {
    // 「几乎同行」：两张水平相邻的卡片，端口纵向只要差几像素，正交候选就只能
    // 画成「横-竖-横」的 Z 形小台阶（箭头看起来不是直的）。这里直接给一条斜线
    // 候选：斜率超过 1/4 或距离过远时不生成，避免长距离斜穿。
    const dy = tp.y - sp.y;
    const dx = tp.x - sp.x;
    const ahead = Math.abs(dx) > 1 && dx * sourceDirection > 0;
    if (ahead && Math.abs(dy) <= Math.min(72, Math.abs(dx) * 0.25)) {
      candidates.push({
        points: [
          [sp.x, sp.y],
          [tp.x, tp.y],
        ],
        penalty: 1 + Math.abs(dy) * 0.15,
        kind: "straight",
      });
    }
  }

  const seen = new Set<number>();
  // 端口之间的中线必须先作为候选：否则当局部 lane 没切出空隙时，会选择
  // 远处的全局 lane，出现「先向右跑很远再折回目标」的夸张绕线。
  const candidateLanes = [...laneXs, (sx + tx) / 2, (sp.x + tp.x) / 2];
  for (const rawLane of candidateLanes) {
    const lane = Math.min(Math.max(rawLane, minLane), maxLane);
    const key = Math.round(lane);
    if (seen.has(key)) continue;
    seen.add(key);
    const backtrack =
      rawLane < minLane - 12 || rawLane > maxLane + 12 ? 32 : 0;
    candidates.push({
      points: [
        [sp.x, sp.y],
        [sx, sp.y],
        [lane, sp.y],
        [lane, tp.y],
        [tx, tp.y],
        [tp.x, tp.y],
      ],
      penalty: backtrack,
      kind: "z",
    });
  }
  return candidates;
}

/** 垂直端口对的 V-H-V 候选：水平段落在全局 lane 上。 */
function buildVerticalCandidates(
  pair: PortPair,
  laneYs: number[],
  stub: number,
): RouteCandidate[] {
  const sp = pair.source;
  const tp = pair.target;
  const sourceDirection = sp.side === "bottom" ? 1 : -1;
  const targetDirection = tp.side === "top" ? -1 : 1;
  const sy = sp.y + sourceDirection * stub;
  const ty = tp.y + targetDirection * stub;
  const minLane = Math.min(sy, ty);
  const maxLane = Math.max(sy, ty);
  const candidates: RouteCandidate[] = [];

  if (Math.abs(sp.x - tp.x) < 1) {
    candidates.push({
      points: [
        [sp.x, sp.y],
        [tp.x, tp.y],
      ],
      penalty: 4,
      kind: "straight",
    });
  } else {
    // 上下相邻、横向只差几像素时同理：允许一条近竖直的斜线，避免同样的 Z 形台阶。
    const dx = tp.x - sp.x;
    const dy = tp.y - sp.y;
    const ahead = Math.abs(dy) > 1 && dy * sourceDirection > 0;
    if (ahead && Math.abs(dx) <= Math.min(72, Math.abs(dy) * 0.25)) {
      candidates.push({
        points: [
          [sp.x, sp.y],
          [tp.x, tp.y],
        ],
        penalty: 1 + Math.abs(dx) * 0.15,
        kind: "straight",
      });
    }
  }

  const seen = new Set<number>();
  // 同水平端口：端口纵坐标中线优先，避免绕到远端全局 lane。
  const candidateLanes = [...laneYs, (sy + ty) / 2, (sp.y + tp.y) / 2];
  for (const rawLane of candidateLanes) {
    const lane = Math.min(Math.max(rawLane, minLane), maxLane);
    const key = Math.round(lane);
    if (seen.has(key)) continue;
    seen.add(key);
    const backtrack =
      rawLane < minLane - 12 || rawLane > maxLane + 12 ? 32 : 0;
    candidates.push({
      points: [
        [sp.x, sp.y],
        [sp.x, sy],
        [sp.x, lane],
        [tp.x, lane],
        [tp.x, ty],
        [tp.x, tp.y],
      ],
      penalty: backtrack,
      kind: "z",
    });
  }
  return candidates;
}

/** 混合方向的 L 形候选（例如源右侧出线、目标上边缘进入）。 */
function buildMixedCandidates(pair: PortPair): RouteCandidate[] {
  const sp = pair.source;
  const tp = pair.target;
  const sourceHorizontal = sp.side === "left" || sp.side === "right";
  const targetHorizontal = tp.side === "left" || tp.side === "right";
  if (sourceHorizontal === targetHorizontal) return [];
  if (sourceHorizontal) {
    return [
      {
        points: [
          [sp.x, sp.y],
          [tp.x, sp.y],
          [tp.x, tp.y],
        ],
        penalty: 34,
        kind: "l",
      },
    ];
  }
  return [
    {
      points: [
        [sp.x, sp.y],
        [sp.x, tp.y],
        [tp.x, tp.y],
      ],
      penalty: 34,
      kind: "l",
    },
  ];
}

/**
 * 通道绕行候选：先到竖直 lane，再走水平通道，最后从目标侧 lane 进入。
 * 用于跨列/向后/被遮挡的边，避免从节点卡片中间穿过。
 */
function buildDetourCandidates(
  pair: PortPair,
  verticalLaneXs: number[],
  horizontalLaneYs: number[],
  bbox: { left: number; top: number; right: number; bottom: number },
  stub: number,
): RouteCandidate[] {
  const sp = pair.source;
  const tp = pair.target;
  const sourceHorizontal = sp.side === "left" || sp.side === "right";
  const targetHorizontal = tp.side === "left" || tp.side === "right";
  const candidates: RouteCandidate[] = [];

  if (sourceHorizontal && targetHorizontal) {
    const sourceDirection = sp.side === "right" ? 1 : -1;
    const targetDirection = tp.side === "right" ? 1 : -1;
    const sx = sp.x + sourceDirection * stub;
    const tx = tp.x + targetDirection * stub;
    const sourceLanes = [...verticalLaneXs]
      .sort((left, right) => Math.abs(left - sx) - Math.abs(right - sx))
      .slice(0, 3);
    const targetLanes = [...verticalLaneXs]
      .sort((left, right) => Math.abs(left - tx) - Math.abs(right - tx))
      .slice(0, 2);
    const corridors = [...horizontalLaneYs]
      .sort(
        (left, right) =>
          Math.abs(left - (sp.y + tp.y) / 2) - Math.abs(right - (sp.y + tp.y) / 2),
      )
      .slice(0, 3);
    for (const corridor of corridors) {
      for (const sourceLane of sourceLanes) {
        for (const targetLane of targetLanes) {
          if (Math.abs(sourceLane - targetLane) < 1) continue;
          candidates.push({
            points: [
              [sp.x, sp.y],
              [sx, sp.y],
              [sourceLane, sp.y],
              [sourceLane, corridor],
              [targetLane, corridor],
              [targetLane, tp.y],
              [tx, tp.y],
              [tp.x, tp.y],
            ],
            penalty: 44,
            kind: "corridor",
          });
        }
      }
    }
    // 整图上下外侧绕行（穿过大量节点时的保底方案）。
    const outerYs = [bbox.top - 42, bbox.bottom + 42];
    for (const outerY of outerYs) {
      const sourceLane = sourceLanes[0] ?? sx;
      const targetLane = targetLanes[0] ?? tx;
      candidates.push({
        points: [
          [sp.x, sp.y],
          [sx, sp.y],
          [sourceLane, sp.y],
          [sourceLane, outerY],
          [targetLane, outerY],
          [targetLane, tp.y],
          [tx, tp.y],
          [tp.x, tp.y],
        ],
        penalty: 260,
        kind: "outer",
      });
    }
    return candidates;
  }

  if (!sourceHorizontal && !targetHorizontal) {
    const sourceDirection = sp.side === "bottom" ? 1 : -1;
    const targetDirection = tp.side === "top" ? -1 : 1;
    const sy = sp.y + sourceDirection * stub;
    const ty = tp.y + targetDirection * stub;
    const sourceLanes = [...horizontalLaneYs]
      .sort((left, right) => Math.abs(left - sy) - Math.abs(right - sy))
      .slice(0, 3);
    const targetLanes = [...horizontalLaneYs]
      .sort((left, right) => Math.abs(left - ty) - Math.abs(right - ty))
      .slice(0, 2);
    const verticals = [...verticalLaneXs]
      .sort(
        (left, right) =>
          Math.abs(left - (sp.x + tp.x) / 2) - Math.abs(right - (sp.x + tp.x) / 2),
      )
      .slice(0, 3);
    for (const vertical of verticals) {
      for (const sourceLane of sourceLanes) {
        for (const targetLane of targetLanes) {
          if (Math.abs(sourceLane - targetLane) < 1) continue;
          candidates.push({
            points: [
              [sp.x, sp.y],
              [sp.x, sy],
              [sp.x, sourceLane],
              [vertical, sourceLane],
              [vertical, targetLane],
              [tp.x, targetLane],
              [tp.x, ty],
              [tp.x, tp.y],
            ],
            penalty: 52,
            kind: "corridor",
          });
        }
      }
    }
    const outerXs = [bbox.left - 42, bbox.right + 42];
    for (const outerX of outerXs) {
      candidates.push({
        points: [
          [sp.x, sp.y],
          [sp.x, sy],
          [outerX, sy],
          [outerX, ty],
          [tp.x, ty],
          [tp.x, tp.y],
        ],
        penalty: 260,
        kind: "outer",
      });
    }
  }
  return candidates;
}

/** 纵向斜线：源右下角 → 目标左上角（简洁顺序分组的「小黄点」连线）。 */
function buildCornerCandidate(
  source: Obstacle,
  target: Obstacle,
  allowDiagonal: boolean,
): RouteCandidate | null {
  const below = target.top >= source.bottom + 2;
  if (!below) return null;
  const columnAligned = Math.abs(target.left - source.left) <= 4;
  if (!columnAligned && !allowDiagonal) return null;

  const sourceWidth = source.right - source.left;
  const targetWidth = target.right - target.left;
  const insetX = Math.min(44, sourceWidth * 0.22, targetWidth * 0.22);
  const start: Point = [source.right - insetX, source.bottom];
  const end: Point = [target.left + insetX, target.top];
  if (columnAligned) {
    return {
      points: [start, end],
      penalty: -24,
      corners: { start, end },
      kind: "corner",
    };
  }
  const midY = (start[1] + end[1]) / 2;
  return {
    points: [start, [start[0], midY], [end[0], midY], end],
    penalty: -18,
    corners: { start, end },
    kind: "corner",
  };
}

/** 障碍外扩一个 padding（标签避让也按外扩后的矩形算，避免贴边）。 */
function inflate(rect: Obstacle, padding: number): Obstacle {
  return {
    ...rect,
    left: rect.left - padding,
    top: rect.top - padding,
    right: rect.right + padding,
    bottom: rect.bottom + padding,
  };
}

/**
 * 在路径线段上为标签选一个「不压卡片」的位置：
 * 优先水平线段（文字更好读），其次长线段、靠近路径中部；碰撞则沿线段滑动。
 */
function chooseLabelSpot(
  points: Point[],
  width: number,
  height: number,
  obstacles: Obstacle[],
): LabelSpot {
  const segments: Array<{
    axis: "horizontal" | "vertical";
    fixed: number;
    start: number;
    end: number;
    index: number;
  }> = [];
  for (let index = 1; index < points.length; index += 1) {
    const a = points[index - 1];
    const b = points[index];
    const horizontal = Math.abs(a[1] - b[1]) < 0.5;
    const vertical = Math.abs(a[0] - b[0]) < 0.5;
    if (horizontal) {
      segments.push({
        axis: "horizontal",
        fixed: a[1],
        start: Math.min(a[0], b[0]),
        end: Math.max(a[0], b[0]),
        index,
      });
    } else if (vertical) {
      segments.push({
        axis: "vertical",
        fixed: a[0],
        start: Math.min(a[1], b[1]),
        end: Math.max(a[1], b[1]),
        index,
      });
    }
  }

  const ordered = [...segments].sort((left, right) => {
    const leftHorizontal = left.axis === "horizontal" ? 0 : 1;
    const rightHorizontal = right.axis === "horizontal" ? 0 : 1;
    if (leftHorizontal !== rightHorizontal) return leftHorizontal - rightHorizontal;
    return right.end - right.start - (left.end - left.start);
  });

  const spotIfFree = (
    segment: (typeof segments)[number],
    position: number,
  ): LabelSpot | null => {
    const size = segment.axis === "horizontal" ? width : height;
    if (position < segment.start + size / 2 + 2) return null;
    if (position > segment.end - size / 2 - 2) return null;
    const centerX = segment.axis === "horizontal" ? position : segment.fixed;
    const centerY = segment.axis === "horizontal" ? segment.fixed : position;
    const labelRect: Obstacle = {
      id: "label",
      left: centerX - width / 2,
      top: centerY - height / 2,
      right: centerX + width / 2,
      bottom: centerY + height / 2,
      isGroup: false,
    };
    for (const obstacle of obstacles) {
      if (overlapsRect(labelRect, obstacle)) return null;
    }
    return {
      x: Math.round(centerX * 10) / 10,
      y: Math.round(centerY * 10) / 10,
      axis: segment.axis,
      min: segment.start + size / 2 + 2,
      max: segment.end - size / 2 - 2,
      collisions: 0,
    };
  };

  // 快速路径：水平线段优先（文字更好读），找到第一个完全不压卡片的点就返回。
  let verticalClean: LabelSpot | null = null;
  for (const segment of ordered) {
    const size = segment.axis === "horizontal" ? width : height;
    const mid = (segment.start + segment.end) / 2;
    const step = Math.max(22, size / 2 + 12);
    const positions: number[] = [mid];
    const half = (segment.end - segment.start) / 2;
    for (let offset = step; offset <= half + step; offset += step) {
      positions.push(mid - offset, mid + offset);
    }
    for (const position of positions) {
      const spot = spotIfFree(segment, position);
      if (!spot) continue;
      if (segment.axis === "horizontal") return spot;
      if (!verticalClean) verticalClean = spot;
    }
  }
  if (verticalClean) return verticalClean;

  // 兜底：没有完全干净的位置，取碰撞最少、最靠线段中部的点（仍在线段上）。
  let best: LabelSpot | null = null;
  let bestScore = Number.POSITIVE_INFINITY;

  for (const segment of ordered) {
    const along = segment.end - segment.start;
    const size = segment.axis === "horizontal" ? width : height;
    const crossSize = segment.axis === "horizontal" ? height : width;
    const mid = (segment.start + segment.end) / 2;
    const positions: number[] = [mid];
    const step = Math.max(24, size / 2 + 14);
    for (let offset = step; offset <= along / 2 + step; offset += step) {
      positions.push(mid - offset, mid + offset);
    }

    for (const rawPosition of positions) {
      if (rawPosition < segment.start + size / 2 + 2) continue;
      if (rawPosition > segment.end - size / 2 - 2) continue;
      const centerX = segment.axis === "horizontal" ? rawPosition : segment.fixed;
      const centerY = segment.axis === "horizontal" ? segment.fixed : rawPosition;
      const labelRect: Obstacle = {
        id: "label",
        left: centerX - width / 2,
        top: centerY - height / 2,
        right: centerX + width / 2,
        bottom: centerY + height / 2,
        isGroup: false,
      };
      let collisions = 0;
      for (const obstacle of obstacles) {
        if (overlapsRect(labelRect, obstacle)) {
          collisions += 1;
          if (collisions >= 2) break;
        }
      }
      const progress = Math.abs(
        (segment.axis === "horizontal" ? centerX : centerY) - mid,
      );
      const score =
        collisions * 900 +
        (segment.axis === "horizontal" ? 0 : 6) +
        (crossSize > 200 ? 30 : 0) +
        progress * 0.05 +
        (along < size + 8 ? 40 : 0) +
        segment.index * 0.3;
      if (score < bestScore) {
        bestScore = score;
        const min = segment.start + size / 2 + 2;
        const max = segment.end - size / 2 - 2;
        best = {
          x: Math.round(centerX * 10) / 10,
          y: Math.round(centerY * 10) / 10,
          axis: segment.axis,
          min: min <= max ? min : (segment.start + segment.end) / 2,
          max: min <= max ? max : (segment.start + segment.end) / 2,
          collisions,
        };
      }
    }
  }

  if (best) return best;

  // 纵向斜线（源右下角 → 目标左上角的特殊连线）：标签压在斜线中点上，
  // 在两端卡片之间的行间空隙里通常正好放得下药丸；沿斜线取几个位置找最干净的。
  if (segments.length === 0) {
    let diagonalBest: LabelSpot | null = null;
    let diagonalScore = Number.POSITIVE_INFINITY;
    for (let index = 1; index < points.length; index += 1) {
      const a = points[index - 1];
      const b = points[index];
      if (Math.abs(a[0] - b[0]) < 0.5 || Math.abs(a[1] - b[1]) < 0.5) continue;
      for (const ratio of [0.5, 0.42, 0.58, 0.34, 0.66]) {
        const centerX = a[0] + (b[0] - a[0]) * ratio;
        const centerY = a[1] + (b[1] - a[1]) * ratio;
        const labelRect: Obstacle = {
          id: "label",
          left: centerX - width / 2,
          top: centerY - height / 2,
          right: centerX + width / 2,
          bottom: centerY + height / 2,
          isGroup: false,
        };
        let collisions = 0;
        for (const obstacle of obstacles) {
          if (overlapsRect(labelRect, obstacle)) {
            collisions += 1;
            if (collisions >= 2) break;
          }
        }
        const score = collisions * 900 + Math.abs(ratio - 0.5) * 120;
        if (score < diagonalScore) {
          diagonalScore = score;
          diagonalBest = {
            x: Math.round(centerX * 10) / 10,
            y: Math.round(centerY * 10) / 10,
            axis: "horizontal",
            min: centerX,
            max: centerX,
            collisions,
          };
        }
      }
    }
    if (diagonalBest) return diagonalBest;
  }

  // 没有可用线段（极短边）：退化为路径中点。
  const mid = points[Math.floor(points.length / 2)] ?? points[0] ?? [0, 0];
  return {
    x: mid[0],
    y: mid[1],
    axis: "horizontal",
    min: mid[0],
    max: mid[0],
    collisions: 0,
  };
}

/** 两个矩形是否相交（严格内部重叠才算是「压住」）。 */
function overlapsRect(a: Obstacle, b: Obstacle): boolean {
  return !(
    a.right <= b.left ||
    a.left >= b.right ||
    a.bottom <= b.top ||
    a.top >= b.bottom
  );
}

/** 同一区域的多个标签沿路径方向错开（保持在线段上，并再次检查障碍）。 */
function staggerLabels(
  internals: Array<{
    spot: LabelSpot;
    size: { width: number; height: number };
    obstacles: Obstacle[];
  }>,
): void {
  const order = internals
    .map((_, index) => index)
    .sort((left, right) => {
      const a = internals[left].spot;
      const b = internals[right].spot;
      return a.x - b.x || a.y - b.y;
    });
  const handled = new Set<number>();
  const placedRects: Obstacle[] = [];

  const overlaps = (rect: Obstacle, other: Obstacle): boolean =>
    !(
      rect.right <= other.left ||
      rect.left >= other.right ||
      rect.bottom <= other.top ||
      rect.top >= other.bottom
    );

  const collides = (rect: Obstacle, obstacles: Obstacle[]): boolean => {
    for (const obstacle of obstacles) {
      if (overlaps(rect, obstacle)) return true;
    }
    for (const other of placedRects) {
      if (overlaps(rect, other)) return true;
    }
    return false;
  };

  for (const currentIndex of order) {
    if (handled.has(currentIndex)) continue;
    const anchor = internals[currentIndex].spot;
    const cluster: number[] = [currentIndex];
    for (const candidateIndex of order) {
      if (candidateIndex === currentIndex || handled.has(candidateIndex)) continue;
      const candidate = internals[candidateIndex].spot;
      if (
        Math.abs(candidate.x - anchor.x) < 78 &&
        Math.abs(candidate.y - anchor.y) < 52
      ) {
        cluster.push(candidateIndex);
      }
    }
    if (cluster.length > 1) {
      cluster.sort(
        (left, right) => internals[left].spot.y - internals[right].spot.y,
      );
      const step = 36;
      cluster.forEach((itemIndex, position) => {
        const item = internals[itemIndex];
        const offset = (position - (cluster.length - 1) / 2) * step;
        const spot = item.spot;
        if (spot.axis === "vertical" && spot.max - spot.min > step / 2) {
          const nextY = Math.min(Math.max(spot.y + offset, spot.min), spot.max);
          if (Math.abs(nextY - spot.y) < 2) return;
          const rect: Obstacle = {
            id: "label",
            left: spot.x - item.size.width / 2,
            top: nextY - item.size.height / 2,
            right: spot.x + item.size.width / 2,
            bottom: nextY + item.size.height / 2,
            isGroup: false,
          };
          if (!collides(rect, item.obstacles)) spot.y = nextY;
        } else if (spot.axis === "horizontal" && spot.max - spot.min > step / 2) {
          const nextX = Math.min(Math.max(spot.x + offset, spot.min), spot.max);
          if (Math.abs(nextX - spot.x) < 2) return;
          const rect: Obstacle = {
            id: "label",
            left: nextX - item.size.width / 2,
            top: spot.y - item.size.height / 2,
            right: nextX + item.size.width / 2,
            bottom: spot.y + item.size.height / 2,
            isGroup: false,
          };
          if (!collides(rect, item.obstacles)) spot.x = nextX;
        }
      });
    }
    for (const itemIndex of cluster) {
      handled.add(itemIndex);
      const spot = internals[itemIndex].spot;
      placedRects.push({
        id: "label",
        left: spot.x - internals[itemIndex].size.width / 2,
        top: spot.y - internals[itemIndex].size.height / 2,
        right: spot.x + internals[itemIndex].size.width / 2,
        bottom: spot.y + internals[itemIndex].size.height / 2,
        isGroup: false,
      });
    }
  }
}

interface ContainerInfo {
  obstacle: Obstacle;
  header: Obstacle;
}

/** 识别分组容器：显式 isGroup 优先，否则用「矩形包含其它节点」启发式。 */
function buildContainers(
  nodes: RoutingNode[],
  obstacles: Map<string, Obstacle>,
): Map<string, ContainerInfo> {
  const containers = new Map<string, ContainerInfo>();
  const rects = [...obstacles.values()];
  for (const node of nodes) {
    const obstacle = obstacles.get(node.id);
    if (!obstacle) continue;
    let isGroup = node.isGroup === true;
    if (!isGroup) {
      isGroup = rects.some(
        (other) =>
          other.id !== obstacle.id &&
          other.right - other.left < obstacle.right - obstacle.left &&
          other.bottom - other.top < obstacle.bottom - obstacle.top &&
          rectContains(obstacle, {
            x: other.left,
            y: other.top,
            width: other.right - other.left,
            height: other.bottom - other.top,
          }),
      );
    }
    if (!isGroup) continue;
    const headerHeight = Math.max(
      12,
      Math.min(
        node.headerHeight ?? FALLBACK_GROUP_HEADER,
        (obstacle.bottom - obstacle.top) / 2,
      ),
    );
    containers.set(node.id, {
      obstacle,
      header: {
        id: `${node.id}::header`,
        left: obstacle.left,
        top: obstacle.top,
        right: obstacle.right,
        bottom: obstacle.top + headerHeight,
        isGroup: true,
      },
    });
  }
  return containers;
}

/** 节点 id -> 包含它的分组容器 id（由外到内）。 */
function buildAncestorMap(
  nodes: RoutingNode[],
  obstacles: Map<string, Obstacle>,
  containers: Map<string, ContainerInfo>,
): Map<string, string[]> {
  const result = new Map<string, string[]>();
  for (const node of nodes) {
    const own = obstacles.get(node.id);
    if (!own) continue;
    const ancestors: Array<{ id: string; area: number }> = [];
    for (const [id, info] of containers) {
      if (id === node.id) continue;
      if (
        rectContains(info.obstacle, {
          x: node.x,
          y: node.y,
          width: node.width,
          height: node.height,
        })
      ) {
        const area =
          (info.obstacle.right - info.obstacle.left) *
          (info.obstacle.bottom - info.obstacle.top);
        ancestors.push({ id, area });
      }
    }
    ancestors.sort((left, right) => right.area - left.area);
    result.set(
      node.id,
      ancestors.map((item) => item.id),
    );
  }
  return result;
}

/** 同组内连线：限制在共同祖先容器内，避免绕到组外再回来。 */
function commonHomeRect(
  ancestorsOf: (id: string) => string[],
  containers: Map<string, ContainerInfo>,
  sourceId: string,
  targetId: string,
): Obstacle | null {
  const sourceAncestors = ancestorsOf(sourceId);
  const targetAncestors = new Set(ancestorsOf(targetId));
  for (const id of sourceAncestors) {
    if (!targetAncestors.has(id)) continue;
    const container = containers.get(id);
    if (container) return container.obstacle;
  }
  return null;
}

function outsidePenalty(points: Point[], home: Obstacle | null): number {
  if (!home) return 0;
  let outside = 0;
  for (const point of points) {
    if (
      point[0] < home.left - 8 ||
      point[0] > home.right + 8 ||
      point[1] < home.top - 8 ||
      point[1] > home.bottom + 8
    ) {
      outside += 1;
    }
  }
  return outside * 55;
}

interface PortAssignment {
  pair: PortPair;
  gap: LaneGap | null;
  slot: number;
}

/**
 * 第一遍：为每条边确定主端口对与要使用的全局通道；同通道内按位置排序分配槽位，
 * 让同向的边分散到不同 lane（结果确定，不依赖遍历顺序）。
 */
function assignPortsAndLanes(
  edges: RoutingEdgeInput[],
  rectsById: Map<string, Obstacle>,
  verticalGaps: LaneGap[],
  horizontalGaps: LaneGap[],
  laneGap: number,
): Map<string, PortAssignment> {
  const preliminary = new Map<string, { pair: PortPair; gap: LaneGap | null }>();
  for (const edge of edges) {
    const source = rectsById.get(edge.sourceId);
    const target = rectsById.get(edge.targetId);
    if (!source || !target) continue;
    const pairs = buildPortPairs(source, target);
    const pair = pairs[0];
    if (!pair) continue;
    const sourceHorizontal = pair.source.side === "left" || pair.source.side === "right";
    const targetHorizontal = pair.target.side === "left" || pair.target.side === "right";
    let gap: LaneGap | null = null;
    if (sourceHorizontal && targetHorizontal) {
      gap = nearestGap(
        verticalGaps,
        (pair.source.x + pair.target.x) / 2,
        pair.source.x,
        pair.target.x,
      )?.gap ?? null;
    } else if (!sourceHorizontal && !targetHorizontal) {
      gap = nearestGap(
        horizontalGaps,
        (pair.source.y + pair.target.y) / 2,
        pair.source.y,
        pair.target.y,
      )?.gap ?? null;
    }
    preliminary.set(edge.id, { pair, gap });
  }

  // 同通道内按源端口 y/x 排序，交替分配槽位。
  const buckets = new Map<string, Array<{ edgeId: string; sortKey: number }>>();
  for (const edge of edges) {
    const entry = preliminary.get(edge.id);
    if (!entry?.gap) continue;
    const sourceHorizontal =
      entry.pair.source.side === "left" || entry.pair.source.side === "right";
    const sortKey = sourceHorizontal
      ? entry.pair.source.y
      : entry.pair.source.x;
    const key = entry.gap.key;
    const bucket = buckets.get(key);
    if (bucket) bucket.push({ edgeId: edge.id, sortKey });
    else buckets.set(key, [{ edgeId: edge.id, sortKey }]);
  }

  const assignments = new Map<string, PortAssignment>();
  for (const edge of edges) {
    const entry = preliminary.get(edge.id);
    if (!entry) continue;
    let slot = 0;
    if (entry.gap) {
      const bucket = buckets.get(entry.gap.key) ?? [];
      bucket.sort((left, right) => left.sortKey - right.sortKey || left.edgeId.localeCompare(right.edgeId));
      const order = bucket.findIndex((item) => item.edgeId === edge.id);
      slot = pickSlot(entry.gap, Math.max(0, order), laneGap);
    }
    assignments.set(edge.id, { pair: entry.pair, gap: entry.gap, slot });
  }
  return assignments;
}

/**
 * 计算一轮边路由（纯函数，可在 rAF 里节流调用）。
 * 复杂度约 O(边数 × 候选数 × 障碍数)；50–100 条边在毫秒级。
 */
export function computeEdgeRoutes(
  nodes: RoutingNode[],
  edges: RoutingEdgeInput[],
  options: RoutingOptions = {},
): Map<string, EdgeRoute> {
  const resolved: Required<RoutingOptions> = {
    stub: options.stub ?? 18,
    padding: options.padding ?? 6,
    radius: options.radius ?? 10,
    laneGap: options.laneGap ?? 14,
  };

  const obstaclesById = new Map<string, Obstacle>();
  for (const node of nodes) {
    obstaclesById.set(node.id, {
      id: node.id,
      left: node.x,
      top: node.y,
      right: node.x + node.width,
      bottom: node.y + node.height,
      isGroup: node.isGroup === true,
      headerHeight: node.headerHeight,
    });
  }
  const containers = buildContainers(nodes, obstaclesById);
  const ancestorsOfMap = buildAncestorMap(nodes, obstaclesById, containers);

  let bbox = {
    left: Number.POSITIVE_INFINITY,
    top: Number.POSITIVE_INFINITY,
    right: Number.NEGATIVE_INFINITY,
    bottom: Number.NEGATIVE_INFINITY,
  };
  const xIntervals: Array<[number, number]> = [];
  const yIntervals: Array<[number, number]> = [];
  for (const rect of obstaclesById.values()) {
    bbox.left = Math.min(bbox.left, rect.left);
    bbox.top = Math.min(bbox.top, rect.top);
    bbox.right = Math.max(bbox.right, rect.right);
    bbox.bottom = Math.max(bbox.bottom, rect.bottom);
    xIntervals.push([rect.left, rect.right]);
    yIntervals.push([rect.top, rect.bottom]);
  }
  if (!Number.isFinite(bbox.left)) {
    bbox = { left: 0, top: 0, right: 0, bottom: 0 };
  }

  const verticalGaps = buildLaneGaps(xIntervals, resolved.laneGap, "v");
  const horizontalGaps = buildLaneGaps(yIntervals, resolved.laneGap, "h");
  const outerVerticalLanes = [
    bbox.left - 40,
    bbox.left - 72,
    bbox.right + 40,
    bbox.right + 72,
  ];
  const outerHorizontalLanes = [
    bbox.top - 40,
    bbox.top - 72,
    bbox.bottom + 40,
    bbox.bottom + 72,
  ];

  const assignments = assignPortsAndLanes(
    edges,
    obstaclesById,
    verticalGaps,
    horizontalGaps,
    resolved.laneGap,
  );

  // 同一张卡片/分组的同一条边可能挂多条连线：沿边把锚点错开，
  // 避免两三段线在卡片边缘完全重合（看起来像一条线连错节点）。
  const sideBuckets = new Map<
    string,
    Array<{ edgeId: string; role: "source" | "target"; sortKey: number; span: number }>
  >();
  for (const edge of edges) {
    const assignment = assignments.get(edge.id);
    if (!assignment) continue;
    const source = obstaclesById.get(edge.sourceId);
    const target = obstaclesById.get(edge.targetId);
    if (!source || !target) continue;
    const pair = assignment.pair;
    const horizontalSide = (side: Side) => side === "left" || side === "right";
    const sourceHorizontal = horizontalSide(pair.source.side);
    const targetHorizontal = horizontalSide(pair.target.side);
    const sourceKey = `${edge.sourceId}::${pair.source.side}`;
    const targetKey = `${edge.targetId}::${pair.target.side}`;
    const sourceItem = {
      edgeId: edge.id,
      role: "source" as const,
      sortKey: sourceHorizontal ? pair.target.y : pair.target.x,
      span: sourceHorizontal ? source.bottom - source.top : source.right - source.left,
    };
    const targetItem = {
      edgeId: edge.id,
      role: "target" as const,
      sortKey: targetHorizontal ? pair.source.y : pair.source.x,
      span: targetHorizontal ? target.bottom - target.top : target.right - target.left,
    };
    for (const [key, item] of [
      [sourceKey, sourceItem],
      [targetKey, targetItem],
    ] as const) {
      const bucket = sideBuckets.get(key);
      if (bucket) bucket.push(item);
      else sideBuckets.set(key, [item]);
    }
  }
  const portOffsets = new Map<string, { source: number; target: number }>();
  for (const [sideKey, items] of sideBuckets) {
    if (items.length < 2) continue;
    items.sort(
      (left, right) =>
        left.sortKey - right.sortKey || left.edgeId.localeCompare(right.edgeId),
    );
    const span = Math.max(24, items[0].span - 20);
    // 卡片左右两侧在渲染上只有一个「端点圆点」（Handle）：任何交错都会让箭头
    // 偏离心点、看起来既不水平也不在点上。水平侧因此不做交错；竖直侧（上下边）
    // 跨度大、没有单点约束，保留 12px 交错避免多条线完全重合。
    const side = sideKey.slice(sideKey.lastIndexOf("::") + 2);
    const maxGap = side === "left" || side === "right" ? 0 : 12;
    const gap = Math.min(maxGap, span / Math.max(1, items.length - 1));
    items.forEach((item, index) => {
      const offset = (index - (items.length - 1) / 2) * gap;
      const entry = portOffsets.get(item.edgeId) ?? { source: 0, target: 0 };
      if (item.role === "source") entry.source = offset;
      else entry.target = offset;
      portOffsets.set(item.edgeId, entry);
    });
  }

  const routes = new Map<string, EdgeRoute>();
  const labelInternals: Array<{
    spot: LabelSpot;
    size: { width: number; height: number };
    route: EdgeRoute;
    obstacles: Obstacle[];
  }> = [];

  for (const edge of edges) {
    const source = obstaclesById.get(edge.sourceId);
    const target = obstaclesById.get(edge.targetId);
    if (!source || !target) continue;

    const ancestorsOf = (id: string) => ancestorsOfMap.get(id) ?? [];
    const ignoreIds = new Set<string>([
      edge.sourceId,
      edge.targetId,
      ...ancestorsOf(edge.sourceId),
      ...ancestorsOf(edge.targetId),
    ]);
    const edgeObstacles: Obstacle[] = [];
    for (const rect of obstaclesById.values()) {
      if (ignoreIds.has(rect.id)) continue;
      edgeObstacles.push(inflate(rect, resolved.padding));
    }
    // 祖先分组的组头条带仍然是障碍：连线不能从自己的组头穿出去。
    for (const id of [...ancestorsOf(edge.sourceId), ...ancestorsOf(edge.targetId)]) {
      const container = containers.get(id);
      if (container) edgeObstacles.push(inflate(container.header, resolved.padding));
    }
    const edgeObstacleIndex = buildObstacleIndex(edgeObstacles);

    const assignment = assignments.get(edge.id);
    const pair = assignment?.pair ?? buildPortPairs(source, target)[0];
    if (!pair) continue;

    const sourceHorizontal = pair.source.side === "left" || pair.source.side === "right";
    const targetHorizontal = pair.target.side === "left" || pair.target.side === "right";
    // 局部通道：只看这条边行进区间内的障碍，保证组内绕行留在组内。
    const laneXs: number[] = [];
    const laneYs: number[] = [];
    if (sourceHorizontal && targetHorizontal) {
      const minY = Math.min(pair.source.y, pair.target.y) - resolved.stub * 2;
      const maxY = Math.max(pair.source.y, pair.target.y) + resolved.stub * 2;
      laneXs.push(...collectLocalLanes(edgeObstacles, minY, maxY, "x", resolved.laneGap));
    }
    if (!sourceHorizontal && !targetHorizontal) {
      const minX = Math.min(pair.source.x, pair.target.x) - resolved.stub * 2;
      const maxX = Math.max(pair.source.x, pair.target.x) + resolved.stub * 2;
      laneYs.push(...collectLocalLanes(edgeObstacles, minX, maxX, "y", resolved.laneGap));
    }
    if (assignment?.gap) laneXs.push(assignment.slot);
    if (assignment?.gap) {
      const gap = assignment.gap;
      for (const slot of gap.slots) laneXs.push(slot);
      laneXs.push((gap.min + gap.max) / 2, gap.min + 8, gap.max - 8);
    }
    if (assignment?.gap && !sourceHorizontal && !targetHorizontal) {
      laneYs.push(assignment.slot);
      const gap = assignment.gap;
      for (const slot of gap.slots) laneYs.push(slot);
      laneYs.push((gap.min + gap.max) / 2, gap.min + 8, gap.max - 8);
      laneYs.push(...outerHorizontalLanes);
    }
    if (sourceHorizontal && targetHorizontal) {
      laneXs.push(...outerVerticalLanes);
    }
    // 候选 lane 裁剪到行进区间附近，控制每条边的候选数量（性能关键）。
    const narrowedLaneXs = narrowLanes(
      laneXs,
      Math.min(pair.source.x, pair.target.x),
      Math.max(pair.source.x, pair.target.x),
    );
    const narrowedLaneYs = narrowLanes(
      laneYs,
      Math.min(pair.source.y, pair.target.y),
      Math.max(pair.source.y, pair.target.y),
    );

    const baseCandidates: RouteCandidate[] = [];
    const candidatePairs = buildPortPairs(source, target);
    const offset = portOffsets.get(edge.id);
    if (offset) {
      const adjusted = buildPortPairs(source, target, offset)[0];
      if (adjusted) candidatePairs[0] = adjusted;
    }
    for (const portPair of candidatePairs) {
      if (portPair.source.side === "left" || portPair.source.side === "right") {
        if (portPair.target.side === "left" || portPair.target.side === "right") {
          baseCandidates.push(
            ...buildHorizontalCandidates(portPair, narrowedLaneXs, resolved.stub).map(
              (candidate) => ({ ...candidate, penalty: candidate.penalty + portPair.penalty }),
            ),
          );
        } else {
          baseCandidates.push(
            ...buildMixedCandidates(portPair).map((candidate) => ({
              ...candidate,
              penalty: candidate.penalty + portPair.penalty,
            })),
          );
        }
      } else if (portPair.target.side === "top" || portPair.target.side === "bottom") {
        baseCandidates.push(
          ...buildVerticalCandidates(portPair, narrowedLaneYs, resolved.stub).map(
            (candidate) => ({ ...candidate, penalty: candidate.penalty + portPair.penalty }),
          ),
        );
      } else {
        baseCandidates.push(
          ...buildMixedCandidates(portPair).map((candidate) => ({
            ...candidate,
            penalty: candidate.penalty + portPair.penalty,
          })),
        );
      }
    }

    // 通道/外侧绕行（主端口对）：优先局部通道，其次全局空隙，最后才是整图外侧。
    // 只有基础候选（直连 / Z / L / 斜线）不够干净时才生成，避免无谓计算。
    const detourPair = candidatePairs[0] ?? pair;
    const buildDetours = (): RouteCandidate[] =>
      buildDetourCandidates(
        detourPair,
        [
          ...collectLocalLanes(
            edgeObstacles,
            Math.min(detourPair.source.y, detourPair.target.y) - resolved.stub,
            Math.max(detourPair.source.y, detourPair.target.y) + resolved.stub,
            "x",
            resolved.laneGap,
          ),
          ...verticalGaps.flatMap((gap) => gap.slots),
          ...outerVerticalLanes,
        ],
        [
          ...collectLocalLanes(
            edgeObstacles,
            Math.min(detourPair.source.x, detourPair.target.x) - resolved.stub,
            Math.max(detourPair.source.x, detourPair.target.x) + resolved.stub,
            "y",
            resolved.laneGap,
          ),
          ...horizontalGaps.flatMap((gap) => gap.slots),
          ...outerHorizontalLanes,
        ],
        bbox,
        resolved.stub,
      );

    // 纵向斜线（顺序展示边的特色连线）：仅在纵向时尝试，加分使其优先被选中。
    const cornerCandidate = buildCornerCandidate(
      source,
      target,
      Boolean(edge.verticalCorner),
    );
    if (cornerCandidate) baseCandidates.push(cornerCandidate);

    const labelWidth = edge.hasLabel ? PILL_WIDTH : DEFAULT_LABEL_WIDTH;
    const labelHeight = edge.hasLabel ? PILL_HEIGHT : DEFAULT_LABEL_HEIGHT;
    const home = commonHomeRect(ancestorsOf, containers, edge.sourceId, edge.targetId);
    // 标签障碍：所有节点（含两端卡片，保证药丸不压卡片）；只忽略祖先分组容器本体。
    const labelIgnoreIds = new Set<string>([
      ...ancestorsOf(edge.sourceId),
      ...ancestorsOf(edge.targetId),
    ]);
    const edgeLabelObstacles: Obstacle[] = [];
    for (const rect of obstaclesById.values()) {
      if (labelIgnoreIds.has(rect.id)) continue;
      edgeLabelObstacles.push(inflate(rect, 2));
    }
    for (const id of [...labelIgnoreIds]) {
      const container = containers.get(id);
      if (container) edgeLabelObstacles.push(inflate(container.header, 2));
    }

    interface ScoredCandidate {
      candidate: RouteCandidate;
      points: Point[];
      collisions: number;
      score: number;
    }
    const picker: { best: ScoredCandidate | null } = { best: null };
    const consider = (candidate: RouteCandidate) => {
      const points = simplifyPoints(candidate.points);
      if (points.length < 2) return;
      const collisions = countCollisionsIndexed(points, edgeObstacleIndex);
      if (collisions > 2) return;
      const score =
        collisions * 1000 +
        candidate.penalty +
        pathBends(points) * 7 +
        pathLength(points) * 0.02 +
        outsidePenalty(points, home);
      const current = picker.best;
      if (!current || score < current.score) {
        picker.best = { candidate, points, collisions, score };
      }
    };

    for (const candidate of baseCandidates) consider(candidate);
    // 短边已经干净且代价低：直接使用，不再生成绕行候选（性能关键路径）。
    const fastEnough =
      picker.best !== null &&
      picker.best.collisions === 0 &&
      picker.best.candidate.penalty <= 60;
    if (!fastEnough) {
      for (const candidate of buildDetours()) consider(candidate);
    }

    // 标签最后放置：先定几何，再在路径线段上找不压卡片的位置。
    let bestRoute: EdgeRoute | null = null;
    let bestSpot: LabelSpot | null = null;
    let bestLabelSize = { width: labelWidth, height: labelHeight };
    const chosen = picker.best;
    if (chosen) {
      const spot = chooseLabelSpot(
        chosen.points,
        labelWidth,
        labelHeight,
        edgeLabelObstacles,
      );
      const end = chosen.points[chosen.points.length - 1];
      const prev = chosen.points[chosen.points.length - 2] ?? end;
      bestSpot = spot;
      bestRoute = {
        path: buildRoundedPath(chosen.points, resolved.radius),
        labelX: spot.x,
        labelY: spot.y,
        endX: end[0],
        endY: end[1],
        endAngle: Math.atan2(end[1] - prev[1], end[0] - prev[0]),
        ...(chosen.candidate.corners
          ? {
              cornerStart: chosen.candidate.corners.start,
              cornerEnd: chosen.candidate.corners.end,
            }
          : {}),
      };
    }

    if (!bestRoute || !bestSpot) {
      // 极端兜底：源右侧 → 目标左侧的标准折线。
      const sp = makePort(source, "right");
      const tp = makePort(target, "left");
      const midX = (sp.x + tp.x) / 2;
      const points: Point[] = [
        [sp.x, sp.y],
        [midX, sp.y],
        [midX, tp.y],
        [tp.x, tp.y],
      ];
      const spot = chooseLabelSpot(points, labelWidth, labelHeight, edgeLabelObstacles);
      bestSpot = spot;
      bestLabelSize = { width: labelWidth, height: labelHeight };
      bestRoute = {
        path: buildRoundedPath(points, resolved.radius),
        labelX: spot.x,
        labelY: spot.y,
        endX: tp.x,
        endY: tp.y,
        endAngle: Math.PI,
      };
    }

    routes.set(edge.id, bestRoute);
    labelInternals.push({
      spot: bestSpot,
      size: bestLabelSize,
      route: bestRoute,
      obstacles: edgeLabelObstacles,
    });
  }

  // 标签错开：只动坐标，始终落在线段上；会再次避开障碍。
  staggerLabels(
    labelInternals.map((item) => ({
      spot: item.spot,
      size: item.size,
      obstacles: item.obstacles,
    })),
  );
  for (const item of labelInternals) {
    item.route.labelX = Math.round(item.spot.x * 10) / 10;
    item.route.labelY = Math.round(item.spot.y * 10) / 10;
  }

  return routes;
}
