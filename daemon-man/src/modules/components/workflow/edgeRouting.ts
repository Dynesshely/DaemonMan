/**
 * 纯函数边路由：
 * - 输入：可见节点（flow 坐标 + 量测宽高）、边（source/target、是否带延时标签）；
 * - 输出：每条边的正交折线 path（圆角）+ 标签坐标（供 EdgeLabelRenderer 药丸定位）。
 *
 * 策略概要：
 * 1. 基本路径：源右侧 → 目标左侧，在列间隙里取竖直段（x 中线）；
 * 2. 同一列间隙里的多条边按 source y 排序分配 lane（稳定顺序、减少交叠）；
 * 3. 撞到节点矩形时：先尝试同间隙内的空闲 lane，再走行间空隙的水平通道 +
 *    全局空闲竖直通道，仍不行则从整图上方/下方外侧绕行；
 * 4. 跨列多、或向后（目标在源左侧）的边优先走通道/外侧，短边优先走列间隙；
 * 5. 同一区域的多个标签沿路径方向错开，且始终落在线段上（药丸不会压卡片）。
 */

export interface RoutingNode {
  id: string;
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface RoutingEdgeInput {
  id: string;
  sourceId: string;
  targetId: string;
  /** 是否为折叠了延时药丸的合成边（需要标签坐标）。 */
  hasLabel?: boolean;
  /** 顺序展示边：允许目标在下方但不同列时也走「小黄点」斜线（更短）。 */
  verticalCorner?: boolean;
}

export interface EdgeRoute {
  /** SVG path（正交折线 + 圆角，flow 坐标）。 */
  path: string;
  labelX: number;
  labelY: number;
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
  /** 同一间隙内的 lane 间距 */
  laneGap?: number;
}

type Point = [number, number];

interface RectObstacle {
  id: string;
  left: number;
  right: number;
  top: number;
  bottom: number;
}

type CandidateKind = "gap" | "corridor" | "outer";

interface Candidate {
  points: Point[];
  kind: CandidateKind;
  penalty: number;
  label: {
    x: number;
    y: number;
    axis: "vertical" | "horizontal";
    /** 标签可移动范围（沿 axis 方向） */
    min: number;
    max: number;
  };
  /** 纵向斜线特殊点位（源右下角 → 目标左上角）。 */
  corners?: { start: Point; end: Point };
}

interface OrderedEdge {
  edge: RoutingEdgeInput;
  sourceY: number;
  targetY: number;
}

const EPSILON = 1.5;
const FAST_PATH_PENALTY = 40;

function nodeCenterY(node: RoutingNode): number {
  return node.y + node.height / 2;
}

function dedupePoints(points: Point[]): Point[] {
  const result: Point[] = [];
  for (const point of points) {
    const prev = result[result.length - 1];
    if (prev && Math.abs(prev[0] - point[0]) < 0.5 && Math.abs(prev[1] - point[1]) < 0.5) {
      continue;
    }
    result.push(point);
  }
  return result;
}

/** 把折线转成带圆角的 SVG path。 */
function buildRoundedPath(rawPoints: Point[], radius: number): string {
  const points = dedupePoints(rawPoints);
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

/** 水平通道：整图行与行之间的空隙（含上下外侧）。 */
function computeCorridorLanes(nodes: RoutingNode[]): number[] {
  if (nodes.length === 0) return [];
  const intervals = nodes
    .map((node) => [node.y, node.y + node.height] as [number, number])
    .sort((left, right) => left[0] - right[0]);
  const merged: Array<[number, number]> = [];
  for (const interval of intervals) {
    const last = merged[merged.length - 1];
    if (last && interval[0] <= last[1] + 8) {
      last[1] = Math.max(last[1], interval[1]);
    } else {
      merged.push([interval[0], interval[1]]);
    }
  }
  const lanes: number[] = [];
  for (let index = 1; index < merged.length; index += 1) {
    const gapTop = merged[index - 1][1];
    const gapBottom = merged[index][0];
    if (gapBottom - gapTop >= 24) {
      lanes.push((gapTop + gapBottom) / 2);
    }
  }
  const top = merged[0][0];
  const bottom = merged[merged.length - 1][1];
  lanes.push(top - 48, top - 66, bottom + 48, bottom + 66);
  return lanes;
}

/** 线段（水平、垂直或斜线）是否穿过障碍矩形（留 EPSILON 容差）。 */
function segmentHitsRect(a: Point, b: Point, rect: RectObstacle): boolean {
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
  obstacles: RectObstacle[],
  ignoreIds: Set<string>,
  cap = 2,
): number {
  let collisions = 0;
  for (let index = 1; index < points.length; index += 1) {
    const a = points[index - 1];
    const b = points[index];
    if (Math.abs(a[0] - b[0]) < 0.5 && Math.abs(a[1] - b[1]) < 0.5) continue;
    for (const rect of obstacles) {
      if (ignoreIds.has(rect.id)) continue;
      if (segmentHitsRect(a, b, rect)) {
        collisions += 1;
        if (collisions >= cap) return collisions;
      }
    }
  }
  return collisions;
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

/** 竖直通道：整图列与列之间的空隙（含左右外侧），任何 y 跨度都不会压到节点。 */
function computeVerticalLanes(nodes: RoutingNode[]): number[] {
  if (nodes.length === 0) return [];
  const intervals = nodes
    .map((node) => [node.x, node.x + node.width] as [number, number])
    .sort((left, right) => left[0] - right[0]);
  const merged: Array<[number, number]> = [];
  for (const interval of intervals) {
    const last = merged[merged.length - 1];
    if (last && interval[0] <= last[1] + 8) {
      last[1] = Math.max(last[1], interval[1]);
    } else {
      merged.push([interval[0], interval[1]]);
    }
  }
  const lanes: number[] = [];
  for (let index = 1; index < merged.length; index += 1) {
    const gapLeft = merged[index - 1][1];
    const gapRight = merged[index][0];
    if (gapRight - gapLeft >= 24) {
      lanes.push((gapLeft + gapRight) / 2);
    }
  }
  lanes.push(merged[0][0] - 40, merged[merged.length - 1][1] + 40);
  return lanes;
}

/** 找离 target 最近的空闲竖直 lane（side 限定在目标点的哪一侧，避免横穿节点）。 */
function nearestLane(
  target: number,
  lanes: number[],
  side: "right" | "left",
  fallback: number,
): number {
  let best: number | null = null;
  let bestDistance = Number.POSITIVE_INFINITY;
  for (const lane of lanes) {
    if (side === "right" && lane <= target) continue;
    if (side === "left" && lane >= target) continue;
    const distance = Math.abs(lane - target);
    if (distance < bestDistance) {
      bestDistance = distance;
      best = lane;
    }
  }
  return best ?? fallback;
}

/** 列间隙候选（含同间隙 lane 分配）。 */
function buildGapCandidates(
  source: RoutingNode,
  target: RoutingNode,
  lane: { index: number; count: number },
  preferCorridor: boolean,
  allowDiagonal: boolean,
  options: Required<RoutingOptions>,
): Candidate[] {
  const { stub, laneGap } = options;
  const sourcePoint: Point = [source.x + source.width, nodeCenterY(source)];
  const targetPoint: Point = [target.x, nodeCenterY(target)];
  const forward = targetPoint[0] > sourcePoint[0] + 8;
  const gapWidth = targetPoint[0] - sourcePoint[0];
  const candidates: Candidate[] = [];

  if (gapWidth > stub * 2) {
    const baseX = (sourcePoint[0] + targetPoint[0]) / 2;
    const minX = sourcePoint[0] + stub;
    const maxX = targetPoint[0] - stub;
    const maxLabelOffset = Math.max(0, gapWidth / 2 - 48);
    const laneXs: number[] = [];
    if (lane.count > 1) {
      const offset = (lane.index - (lane.count - 1) / 2) * laneGap;
      laneXs.push(
        baseX + Math.min(Math.max(offset, -maxLabelOffset), maxLabelOffset),
      );
    }
    laneXs.push(baseX, baseX - laneGap, baseX + laneGap);
    const seen = new Set<number>();
    for (const rawX of laneXs) {
      const x = Math.min(Math.max(rawX, minX), maxX);
      const key = Math.round(x);
      if (seen.has(key)) continue;
      seen.add(key);
      candidates.push({
        points: [
          [sourcePoint[0], sourcePoint[1]],
          [x, sourcePoint[1]],
          [x, targetPoint[1]],
          [targetPoint[0], targetPoint[1]],
        ],
        kind: "gap",
        penalty: preferCorridor ? 55 : 0,
        label: {
          x,
          y: (sourcePoint[1] + targetPoint[1]) / 2,
          axis: "vertical",
          min: Math.min(sourcePoint[1], targetPoint[1]) + 22,
          max: Math.max(sourcePoint[1], targetPoint[1]) - 22,
        },
      });
    }
  }

  if (Math.abs(sourcePoint[1] - targetPoint[1]) < 1.5 && forward) {
    candidates.push({
      points: [sourcePoint, targetPoint],
      kind: "gap",
      penalty: preferCorridor ? 50 : 2,
      label: {
        x: (sourcePoint[0] + targetPoint[0]) / 2,
        y: sourcePoint[1],
        axis: "horizontal",
        min: sourcePoint[0] + 52,
        max: targetPoint[0] - 52,
      },
    });
  }

  // 纵向排列（目标在下方）：走「源底部靠右 → 目标顶部靠左」的斜线直连。
  // 普通边仅在同一列时启用；顺序展示边（compact 分组的 seq 边）允许跨列，
  // 这样行末 → 下一行行首不用从组外绕一大圈。
  const below = target.y >= source.y + source.height + 2;
  const verticallyStacked =
    below && (Math.abs(target.x - source.x) <= 2 || allowDiagonal);
  if (verticallyStacked) {
    const insetX = Math.min(44, source.width * 0.22, target.width * 0.22);
    const insetY = 0;
    const start: Point = [
      source.x + source.width - insetX,
      source.y + source.height - insetY,
    ];
    const end: Point = [target.x + insetX, target.y + insetY];
    candidates.push({
      points: [start, end],
      kind: "gap",
      penalty: -20,
      corners: { start, end },
      label: {
        x: (start[0] + end[0]) / 2,
        y: (start[1] + end[1]) / 2,
        axis: "horizontal",
        min: Math.min(start[0], end[0]) + 48,
        max: Math.max(start[0], end[0]) - 48,
      },
    });
  }
  return candidates;
}

/** 通道 / 外侧绕行候选：竖直段放在全局空闲 lane 上，水平段走行间空隙。 */
function buildDetourCandidates(
  source: RoutingNode,
  target: RoutingNode,
  corridors: number[],
  verticalLanes: number[],
  bbox: { top: number; bottom: number },
  preferCorridor: boolean,
  options: Required<RoutingOptions>,
): Candidate[] {
  const { stub } = options;
  const sourcePoint: Point = [source.x + source.width, nodeCenterY(source)];
  const targetPoint: Point = [target.x, nodeCenterY(target)];
  const candidates: Candidate[] = [];

  const sourceLaneX = nearestLane(
    sourcePoint[0],
    verticalLanes,
    "right",
    sourcePoint[0] + stub,
  );
  const targetLaneX = nearestLane(
    targetPoint[0],
    verticalLanes,
    "left",
    targetPoint[0] - stub,
  );

  const buildPoints = (laneY: number): Point[] => [
    sourcePoint,
    [sourceLaneX, sourcePoint[1]],
    [sourceLaneX, laneY],
    [targetLaneX, laneY],
    [targetLaneX, targetPoint[1]],
    targetPoint,
  ];

  const pushCandidate = (laneY: number, kind: CandidateKind, penalty: number) => {
    candidates.push({
      points: buildPoints(laneY),
      kind,
      penalty,
      label: {
        x: (sourceLaneX + targetLaneX) / 2,
        y: laneY,
        axis: "horizontal",
        min: Math.min(sourceLaneX, targetLaneX) + 44,
        max: Math.max(sourceLaneX, targetLaneX) - 44,
      },
    });
  };

  const midpointY = (sourcePoint[1] + targetPoint[1]) / 2;
  const orderedCorridors = [...corridors].sort(
    (left, right) => Math.abs(left - midpointY) - Math.abs(right - midpointY),
  );
  orderedCorridors.slice(0, 3).forEach((laneY, index) => {
    pushCandidate(laneY, "corridor", (preferCorridor ? 18 : 70) + index * 6);
  });

  pushCandidate(bbox.top - 34, "outer", preferCorridor ? 42 : 95);
  pushCandidate(bbox.bottom + 34, "outer", preferCorridor ? 50 : 103);

  return candidates;
}

/** 构造「同一列间隙里的边」lane 顺序：按源 y、目标 y 排序，保持稳定、减少交叉。 */
function assignLanes(
  edges: RoutingEdgeInput[],
  nodeById: Map<string, RoutingNode>,
): Map<string, { index: number; count: number }> {
  const groups = new Map<string, OrderedEdge[]>();
  for (const edge of edges) {
    const source = nodeById.get(edge.sourceId);
    const target = nodeById.get(edge.targetId);
    if (!source || !target) continue;
    const key = `${Math.round((source.x + source.width) / 48)}:${Math.round(target.x / 48)}`;
    const bucket = groups.get(key);
    const item: OrderedEdge = {
      edge,
      sourceY: nodeCenterY(source),
      targetY: nodeCenterY(target),
    };
    if (bucket) bucket.push(item);
    else groups.set(key, [item]);
  }
  const result = new Map<string, { index: number; count: number }>();
  for (const bucket of groups.values()) {
    bucket.sort(
      (left, right) =>
        left.sourceY - right.sourceY ||
        left.targetY - right.targetY ||
        left.edge.id.localeCompare(right.edge.id),
    );
    bucket.forEach((item, index) => {
      result.set(item.edge.id, { index, count: bucket.length });
    });
  }
  return result;
}

/** 同一区域的多个标签沿路径错开（仍保持在线段上）。 */
function staggerLabels(
  internals: Array<{ route: EdgeRoute; label: Candidate["label"] }>,
): void {
  const labels = [...internals].sort(
    (left, right) =>
      left.route.labelX - right.route.labelX ||
      left.route.labelY - right.route.labelY,
  );
  const handled = new Set<number>();

  for (let index = 0; index < labels.length; index += 1) {
    if (handled.has(index)) continue;
    const cluster: number[] = [index];
    const anchor = labels[index].route;
    for (let next = index + 1; next < labels.length; next += 1) {
      if (handled.has(next)) continue;
      const candidate = labels[next].route;
      if (
        Math.abs(candidate.labelX - anchor.labelX) < 70 &&
        Math.abs(candidate.labelY - anchor.labelY) < 46
      ) {
        cluster.push(next);
      }
    }
    if (cluster.length <= 1) continue;
    handled.add(index);
    for (const item of cluster.slice(1)) handled.add(item);

    cluster.sort(
      (left, right) => labels[left].route.labelY - labels[right].route.labelY,
    );
    const step = 38;
    cluster.forEach((itemIndex, position) => {
      const item = labels[itemIndex];
      const offset = (position - (cluster.length - 1) / 2) * step;
      if (item.label.axis === "vertical") {
        if (item.label.max - item.label.min > step / 2) {
          item.route.labelY = Math.min(
            Math.max(item.route.labelY + offset, item.label.min),
            item.label.max,
          );
        }
      } else if (item.label.max - item.label.min > step / 2) {
        item.route.labelX = Math.min(
          Math.max(item.route.labelX + offset, item.label.min),
          item.label.max,
        );
      }
    });
  }
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

  const nodeById = new Map<string, RoutingNode>();
  const obstacles: RectObstacle[] = [];
  let minY = Number.POSITIVE_INFINITY;
  let maxY = Number.NEGATIVE_INFINITY;
  let minX = Number.POSITIVE_INFINITY;
  let maxX = Number.NEGATIVE_INFINITY;
  for (const node of nodes) {
    nodeById.set(node.id, node);
    obstacles.push({
      id: node.id,
      left: node.x - resolved.padding,
      right: node.x + node.width + resolved.padding,
      top: node.y - resolved.padding,
      bottom: node.y + node.height + resolved.padding,
    });
    minY = Math.min(minY, node.y);
    maxY = Math.max(maxY, node.y + node.height);
    minX = Math.min(minX, node.x);
    maxX = Math.max(maxX, node.x + node.width);
  }
  if (!Number.isFinite(minY)) {
    minY = 0;
    maxY = 0;
    minX = 0;
    maxX = 0;
  }
  const bbox = { top: minY, bottom: maxY, left: minX, right: maxX };
  const corridors = computeCorridorLanes(nodes);
  const verticalLanes = computeVerticalLanes(nodes);
  const lanes = assignLanes(edges, nodeById);

  const routes = new Map<string, EdgeRoute>();
  const internals: Array<{ route: EdgeRoute; label: Candidate["label"] }> = [];

  const scoreOf = (candidate: Candidate, collisions: number): number =>
    collisions * 1000 +
    candidate.penalty +
    (candidate.points.length - 2) * 4 +
    pathLength(candidate.points) * 0.01;

  for (const edge of edges) {
    const source = nodeById.get(edge.sourceId);
    const target = nodeById.get(edge.targetId);
    if (!source || !target) continue;
    const ignoreIds = new Set([edge.sourceId, edge.targetId]);
    const lane = lanes.get(edge.id) ?? { index: 0, count: 1 };
    const forward = target.x > source.x + source.width;
    const columnSpan =
      Math.abs(source.x + source.width / 2 - (target.x + target.width / 2)) /
      Math.max(1, source.width);
    const preferCorridor = !forward || columnSpan > 2.2;

    let bestCandidate: Candidate | null = null;
    let bestScore = Number.POSITIVE_INFINITY;
    let bestCollisions = 0;

    const gapCandidates = buildGapCandidates(
      source,
      target,
      lane,
      preferCorridor,
      Boolean(edge.verticalCorner),
      resolved,
    );
    for (const candidate of gapCandidates) {
      const collisions = countCollisions(
        candidate.points,
        obstacles,
        ignoreIds,
      );
      const score = scoreOf(candidate, collisions);
      if (score < bestScore) {
        bestScore = score;
        bestCollisions = collisions;
        bestCandidate = candidate;
      }
    }

    // 短边 + 空隙内干净路径：直接使用，不再生成绕行候选（性能关键路径）。
    const fastEnough =
      bestCandidate &&
      bestCollisions === 0 &&
      bestCandidate.penalty < FAST_PATH_PENALTY;

    if (!fastEnough) {
      const detours = buildDetourCandidates(
        source,
        target,
        corridors,
        verticalLanes,
        bbox,
        preferCorridor,
        resolved,
      );
      for (const candidate of detours) {
        const collisions = countCollisions(
          candidate.points,
          obstacles,
          ignoreIds,
        );
        const score = scoreOf(candidate, collisions);
        if (score < bestScore) {
          bestScore = score;
          bestCandidate = candidate;
        }
      }
    }

    if (!bestCandidate) continue;
    const route: EdgeRoute = {
      path: buildRoundedPath(bestCandidate.points, resolved.radius),
      labelX: Math.round(bestCandidate.label.x * 10) / 10,
      labelY: Math.round(bestCandidate.label.y * 10) / 10,
      ...(bestCandidate.corners
        ? {
            cornerStart: bestCandidate.corners.start,
            cornerEnd: bestCandidate.corners.end,
          }
        : {}),
    };
    routes.set(edge.id, route);
    internals.push({ route, label: bestCandidate.label });
  }

  staggerLabels(internals);
  return routes;
}
