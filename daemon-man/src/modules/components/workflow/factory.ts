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
import { DELAY_PILL_HEIGHT, DELAY_PILL_WIDTH } from "./delayPill";

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
  };
}

/** draft 中的节点 -> 画布节点（type 直接用 kind；候选 delay 渲染成连线药丸）。 */
export function buildFlowNode(
  node: WorkflowNode,
  groupSize?: { width: number; height: number },
  pill = false,
): Node<FlowNodeData> {
  if (pill && node.kind === "delay") {
    return {
      id: node.id,
      type: "delay-pill",
      position: { x: node.position.x, y: node.position.y },
      data: { node },
      // 药丸锁定在连线上：不可拖动、不可连线，但可选中 / 删除 / 单点执行。
      draggable: false,
      connectable: false,
      selectable: true,
      style: {
        width: `${DELAY_PILL_WIDTH}px`,
        height: `${DELAY_PILL_HEIGHT}px`,
      },
    };
  }

  const base: Node<FlowNodeData> = {
    id: node.id,
    type: node.kind,
    position: { x: node.position.x, y: node.position.y },
    data: { node },
  };

  if (node.kind !== "group") return base;

  // 分组节点是可拖动的容器背景：拖动时整组（含嵌套后代）跟随平移。
  // 子节点在层级与 DOM 顺序上都位于分组之上，所以分组不会遮挡子节点交互。
  const width = groupSize?.width ?? DEFAULT_GROUP_WIDTH;
  const height = groupSize?.height ?? DEFAULT_GROUP_HEIGHT;
  return {
    ...base,
    width,
    height,
    // 置底：保证后创建的分组也不会盖住已有子节点（分组只是背景容器）。
    zIndex: -1,
    draggable: true,
    connectable: false,
    selectable: true,
    style: {
      width: `${width}px`,
      height: `${height}px`,
    },
  };
}

/** draft 中的边 -> 画布连线（flow=true 执行流，false 依赖跳线）。 */
export function buildFlowEdge(
  edge: WorkflowEdge,
  dark: boolean,
  flow = true,
): Edge<FlowEdgeData> {
  const color = flow
    ? dark
      ? "#818cf8"
      : "#6366f1"
    : dark
      ? "#71717a"
      : "#a1a1aa";
  return {
    id: edge.id,
    source: edge.source,
    target: edge.target,
    type: "workflow",
    data: { edge, flow },
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
