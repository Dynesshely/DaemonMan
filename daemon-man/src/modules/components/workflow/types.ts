import type { Component, InjectionKey, Ref } from "vue";

import type { NodeKind, WorkflowEdge, WorkflowNode } from "@/api";
import type { EdgeRoute } from "./edgeRouting";

/**
 * Vue Flow 自定义节点的 `data`。
 * 直接持有 `store.draft` 中的节点对象，因此属性面板里的改动无需重建画布即可实时反映到节点卡片。
 */
export interface FlowNodeData {
  node: WorkflowNode;
}

/**
 * Vue Flow 自定义连线的 `data`。
 * 同样直接持有 draft 中的边对象，条件改动会实时反映到连线标签。
 * 带 `delayNodeId` 的边是「延时折叠」合成边，药丸在边标签里渲染。
 */
export interface FlowEdgeData {
  edge: WorkflowEdge;
  /** true = 执行流（相邻层 success），false = 依赖跳线 */
  flow: boolean;
  /** 该边折叠的延时节点 id（存在时渲染药丸标签，且点击边会选中该节点）。 */
  delayNodeId?: string;
  /** 延时毫秒数（药丸文案）。 */
  delayMs?: number;
  /** true = 纯展示的合成边（不写 draft，不参与拓扑/删除/选中）。 */
  displayOnly?: boolean;
  /** true = 简洁顺序分组的「执行顺序」连线（displayOnly 的一种）。 */
  sequence?: boolean;
}

/** 画布向下游节点卡片提供「执行步骤号」。 */
export const WORKFLOW_STEPS: InjectionKey<Ref<Map<string, number>>> =
  Symbol("workflow-steps");

/** 画布向下游边组件提供「当前选中的节点 id」（折叠药丸不渲染节点，需要外部选中态高亮）。 */
export const WORKFLOW_SELECTED_NODE: InjectionKey<Ref<string | null>> =
  Symbol("workflow-selected-node");

/** 画布向下游边组件提供「一轮算好的边路由」（正交 path + 标签坐标）。 */
export const WORKFLOW_EDGE_ROUTES: InjectionKey<Ref<Map<string, EdgeRoute>>> =
  Symbol("workflow-edge-routes");

/** 四种节点类型的展示元数据（图标、i18n key、强调色）。 */
export interface NodeKindMeta {
  kind: NodeKind;
  icon: Component;
  /** 节点类型名：node.kind.* */
  labelKey: string;
  /** 节点面板按钮文案：workflow.add* */
  addLabelKey: string;
  /** 节点面板按钮 tooltip：workflow.add*Hint */
  hintKey: string;
  /** 图标底色 / 前景色 */
  iconClass: string;
}

/** 画布 -> 外层视图的选中状态回传。 */
export interface WorkflowSelection {
  nodeId: string | null;
  edgeId: string | null;
}

/** 编辑器中间区域的两种视图。 */
export type WorkflowViewMode = "nodes" | "text";
