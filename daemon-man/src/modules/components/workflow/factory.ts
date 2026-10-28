import { MarkerType } from "@vue-flow/core";
import type { Edge, Node } from "@vue-flow/core";

import type {
  NodeKind,
  NodePosition,
  WorkflowEdge,
  WorkflowNode,
} from "@/api";

import type { FlowEdgeData, FlowNodeData } from "./types";
import { DEFAULT_GROUP_HEIGHT, DEFAULT_GROUP_WIDTH } from "./layout";

/** crypto.randomUUID 的降级实现（与 store 中的 uuid 保持一致的行为）。 */
export function createNodeId(): string {
  if (typeof crypto !== "undefined" && "randomUUID" in crypto) {
    return crypto.randomUUID();
  }
  return `${Date.now().toString(16)}-${Math.random().toString(16).slice(2)}`;
}

/**
 * 按后端 `WorkflowNode::default()` 的语义构造新节点。
 * - application：path 空、skipIfRunning=true、waitForWindow=false、窗口超时 15000ms
 * - delay：1000ms
 * - wait：timeoutMs=30000、failOnTimeout=true
 * - script：timeoutMs=60000、failOnNonzero=true
 */
export function createWorkflowNode(
  kind: NodeKind,
  position: NodePosition,
): WorkflowNode {
  return {
    id: createNodeId(),
    name: "",
    kind,
    enabled: true,
    position,
    app: {
      path: "",
      args: [],
      workingDir: null,
      env: [],
      skipIfRunning: true,
      processName: null,
      waitForWindow: false,
      windowTimeoutMs: 15_000,
      closeMainWindow: false,
      focusWindow: false,
      runAsAdmin: false,
      settleMs: 0,
    },
    delayMs: kind === "delay" ? 1_000 : 0,
    wait: {
      processName: "",
      windowVisible: false,
      timeoutMs: 30_000,
      failOnTimeout: true,
    },
    script: {
      command: "",
      workingDir: null,
      timeoutMs: 60_000,
      failOnNonzero: true,
    },
    note: "",
    parentId: null,
    groupParallel: false,
    groupMode: null,
    groupDelayMs: 0,
    groupParallelLimit: 0,
    groupParallelIntervalMs: 0,
    groupCompact: true,
  };
}

/** draft 中的节点 -> 画布节点（type 直接用 kind）。 */
export function buildFlowNode(
  node: WorkflowNode,
  groupSize?: { width: number; height: number },
  groupDepth = 0,
): Node<FlowNodeData> {
  const base: Node<FlowNodeData> = {
    id: node.id,
    type: node.kind,
    position: { x: node.position.x, y: node.position.y },
    data: { node },
  };

  if (node.kind !== "group") return base;

  // 分组节点是可拖动的容器背景：拖动时整组（含嵌套后代）跟随平移。
  // zIndex 按嵌套深度分层（外层低、内层高，全部 < 0），保证内层组头不被外层容器遮挡，
  // 同时所有分组仍在普通卡片（zIndex 0）下方。
  const width = groupSize?.width ?? DEFAULT_GROUP_WIDTH;
  const height = groupSize?.height ?? DEFAULT_GROUP_HEIGHT;
  return {
    ...base,
    width,
    height,
    zIndex: groupDepth - 1000,
    draggable: true,
    connectable: true,
    selectable: true,
    style: {
      width: `${width}px`,
      height: `${height}px`,
    },
  };
}

/** 合成连线上的延时标签数据（药丸由边组件的 EdgeLabelRenderer 渲染）。 */
export interface DelayEdgeLabel {
  delayNodeId: string;
  delayMs: number;
}

/** 纯展示合成边的构造选项（不写 draft，不可选中/删除/聚焦）。 */
export interface DisplayEdgeOptions {
  /** sequence=true 表示「简洁顺序分组的执行顺序连线」。 */
  sequence?: boolean;
}

/** draft 中的边 -> 画布连线（flow=true 执行流，false 依赖跳线；延时折叠边带药丸数据）。 */
export function buildFlowEdge(
  edge: WorkflowEdge,
  dark: boolean,
  flow = true,
  delay?: DelayEdgeLabel,
  display?: DisplayEdgeOptions,
): Edge<FlowEdgeData> {
  const color = flow
    ? dark
      ? "#818cf8"
      : "#6366f1"
    : dark
      ? "#71717a"
      : "#a1a1aa";
  const displayOnly = Boolean(display);
  return {
    id: edge.id,
    source: edge.source,
    target: edge.target,
    type: "workflow",
    // 固定 0：分组节点的 zIndex 为负，不能让分组间连线继承负值而沉到画布下面。
    zIndex: 0,
    data: {
      edge,
      flow,
      ...(delay
        ? { delayNodeId: delay.delayNodeId, delayMs: delay.delayMs }
        : {}),
      ...(displayOnly
        ? { displayOnly: true, sequence: Boolean(display?.sequence) }
        : {}),
    },
    // 展示边不参与任何交互，避免被选中/删除/编辑。
    ...(displayOnly
      ? { selectable: false, deletable: false, focusable: false }
      : {}),
    markerEnd: {
      type: MarkerType.ArrowClosed,
      color,
      width: flow ? 18 : 14,
      height: flow ? 18 : 14,
    },
  };
}

/** 由 source/target 生成稳定且可读的边 id（同一对节点不允许重复连线）。 */
export function edgeIdFor(source: string, target: string): string {
  return `edge-${source}-${target}`;
}

/** 文件扩展名（小写，不带点）；没有扩展名返回空串。 */
export function fileExtension(path: string): string {
  const name = path.split(/[\\/]/).pop() ?? "";
  const dot = name.lastIndexOf(".");
  return dot >= 0 ? name.slice(dot + 1).toLowerCase() : "";
}

/** 去掉扩展名的文件名（用于拖放建节点的默认名称）。 */
export function fileBaseName(path: string): string {
  const name = path.split(/[\\/]/).pop() ?? path;
  const dot = name.lastIndexOf(".");
  return dot > 0 ? name.slice(0, dot) : name;
}
