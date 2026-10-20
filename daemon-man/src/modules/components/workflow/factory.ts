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
  };
}

/** draft 中的节点 -> 画布节点（type 直接用 kind，配合自定义节点槽位）。 */
export function buildFlowNode(
  node: WorkflowNode,
  groupSize?: { width: number; height: number },
): Node<FlowNodeData> {
  const base: Node<FlowNodeData> = {
    id: node.id,
    type: node.kind,
    position: { x: node.position.x, y: node.position.y },
    data: { node },
  };

  if (node.kind !== "group") return base;

  // 分组节点只是容器背景层：置底、不可拖拽/连线，整块背景不拦截指针事件，
  // 只有标题栏单独开启 pointer-events，避免遮挡子节点与画布平移。
  const width = groupSize?.width ?? DEFAULT_GROUP_WIDTH;
  const height = groupSize?.height ?? DEFAULT_GROUP_HEIGHT;
  return {
    ...base,
    width,
    height,
    zIndex: 0,
    draggable: false,
    connectable: false,
    selectable: true,
    style: {
      width: `${width}px`,
      height: `${height}px`,
      pointerEvents: "none",
    },
  };
}

/** draft 中的边 -> 画布连线。 */
export function buildFlowEdge(
  edge: WorkflowEdge,
  dark: boolean,
): Edge<FlowEdgeData> {
  return {
    id: edge.id,
    source: edge.source,
    target: edge.target,
    type: "workflow",
    data: { edge },
    markerEnd: {
      type: MarkerType.ArrowClosed,
      color: dark ? "#818cf8" : "#6366f1",
      width: 18,
      height: 18,
    },
  };
}

/** 由 source/target 生成稳定且可读的边 id（同一对节点不允许重复连线）。 */
export function edgeIdFor(source: string, target: string): string {
  return `edge-${source}-${target}`;
}
