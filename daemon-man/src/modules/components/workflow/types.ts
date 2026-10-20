import type { Component } from "vue";

import type { NodeKind, WorkflowEdge, WorkflowNode } from "@/api";

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
 */
export interface FlowEdgeData {
  edge: WorkflowEdge;
}

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
