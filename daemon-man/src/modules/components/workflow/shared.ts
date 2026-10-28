/** 工作流编辑器内复用的面板样式与错误文本工具。 */
import type { GroupMode, WorkflowNode } from "@/api";

/** 左栏 / 右栏 / 运行历史的外框。 */
export const panelClass =
  "flex min-h-0 flex-col overflow-hidden rounded-xl border border-zinc-200 bg-white shadow-sm dark:border-zinc-800 dark:bg-zinc-900/60";

/** 面板头部（标题 + 操作）。 */
export const panelHeaderClass =
  "flex shrink-0 items-center justify-between gap-2 border-b border-zinc-200/80 px-3 py-2.5 dark:border-zinc-800/80";

/** 小号全大写小节标题。 */
export const sectionLabelClass =
  "text-[11px] font-semibold uppercase tracking-wider text-zinc-500 dark:text-zinc-400";

/** 折叠块 / 字段分组的外框。 */
export const fieldGroupClass =
  "rounded-lg border border-zinc-200/80 px-3 py-2.5 dark:border-zinc-800/80";

/** 把 IPC / Error 抛出的任意错误转成可展示文本。 */
export function errorText(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === "string") return error;
  return String(error);
}

/**
 * 分组实际启动模式：新字段 `groupMode` 优先，旧数据为 null 时按 `groupParallel` 推导。
 */
export function groupModeOf(node: WorkflowNode): GroupMode {
  if (node.groupMode) return node.groupMode;
  return node.groupParallel ? "parallel" : "sequence";
}

/**
 * 是否「顺序 + 简洁展示」分组：顺序模式默认开启简洁展示
 * （`groupCompact` 缺省/undefined 视为 true，兼容旧数据）。
 */
export function isCompactGroup(node: WorkflowNode): boolean {
  return (
    node.kind === "group" &&
    groupModeOf(node) === "sequence" &&
    node.groupCompact !== false
  );
}

/** 切换分组模式：同步兼容字段 `groupParallel`；切到 sequence 默认开启简洁展示。 */
export function applyGroupMode(node: WorkflowNode, mode: GroupMode): void {
  node.groupMode = mode;
  node.groupParallel = mode === "parallel";
  if (mode === "sequence") node.groupCompact = true;
}

/** 节点最近的「简洁展示」分组祖先 id（用于隐藏组内连线 / 拖拽排序判定）。 */
export function compactAncestorOf(
  nodeId: string,
  nodes: WorkflowNode[],
): string | null {
  const byId = new Map(nodes.map((node) => [node.id, node]));
  const seen = new Set<string>();
  let current = byId.get(nodeId);
  while (current && current.parentId && !seen.has(current.id)) {
    seen.add(current.id);
    const parent = byId.get(current.parentId);
    if (!parent) return null;
    if (isCompactGroup(parent)) return parent.id;
    current = parent;
  }
  return null;
}

/**
 * 分组的嵌套深度（顶层分组 = 0）：沿 parentId 数上方分组层数，成环时停止。
 * 用于画布 zIndex 分层（外层低、内层高）。
 */
export function groupDepthOf(node: WorkflowNode, nodes: WorkflowNode[]): number {
  const byId = new Map(nodes.map((item) => [item.id, item]));
  const seen = new Set<string>([node.id]);
  let depth = 0;
  let current = node.parentId ? byId.get(node.parentId) : null;
  while (current && !seen.has(current.id)) {
    seen.add(current.id);
    if (current.kind === "group") depth += 1;
    current = current.parentId ? byId.get(current.parentId) : null;
  }
  return depth;
}

/**
 * 分组节点的画布 zIndex：按嵌套深度分层（外层更负、内层更大），
 * 但始终 < 0，保证分组背景永远在普通卡片（zIndex 0）下方。
 */
export function groupZIndexOf(node: WorkflowNode, nodes: WorkflowNode[]): number {
  return groupDepthOf(node, nodes) - 1000;
}
