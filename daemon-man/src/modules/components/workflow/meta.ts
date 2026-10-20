import { AppWindow, Hourglass, SquareTerminal, Timer } from "lucide-vue-next";

import type { EdgeCondition, NodeKind, NodeStatus } from "@/api";

import type { NodeKindMeta } from "./types";

/** 节点面板与自动排列的顺序。 */
export const nodeKindOrder: NodeKind[] = [
  "application",
  "delay",
  "wait",
  "script",
];

export const nodeKindMeta: Record<NodeKind, NodeKindMeta> = {
  application: {
    kind: "application",
    icon: AppWindow,
    labelKey: "node.kind.application",
    addLabelKey: "workflow.addApp",
    hintKey: "workflow.addAppHint",
    iconClass:
      "bg-brand-50 text-brand-600 dark:bg-brand-500/15 dark:text-brand-300",
  },
  delay: {
    kind: "delay",
    icon: Timer,
    labelKey: "node.kind.delay",
    addLabelKey: "workflow.addDelay",
    hintKey: "workflow.addDelayHint",
    iconClass: "bg-sky-50 text-sky-600 dark:bg-sky-500/15 dark:text-sky-300",
  },
  wait: {
    kind: "wait",
    icon: Hourglass,
    labelKey: "node.kind.wait",
    addLabelKey: "workflow.addWait",
    hintKey: "workflow.addWaitHint",
    iconClass:
      "bg-amber-50 text-amber-600 dark:bg-amber-500/15 dark:text-amber-300",
  },
  script: {
    kind: "script",
    icon: SquareTerminal,
    labelKey: "node.kind.script",
    addLabelKey: "workflow.addScript",
    hintKey: "workflow.addScriptHint",
    iconClass:
      "bg-violet-50 text-violet-600 dark:bg-violet-500/15 dark:text-violet-300",
  },
};

/** 运行状态 -> 节点卡片边框色。 */
export const nodeStatusBorder: Record<NodeStatus, string> = {
  pending: "border-zinc-200 dark:border-zinc-700",
  waiting: "border-sky-400/70 dark:border-sky-500/50",
  running: "border-brand-500 dark:border-brand-400",
  success: "border-emerald-500/80 dark:border-emerald-400/70",
  failed: "border-red-500/85 dark:border-red-400/75",
  skipped: "border-zinc-300 dark:border-zinc-700",
  cancelled: "border-amber-500/85 dark:border-amber-400/75",
};

/** 运行状态 -> 状态圆点颜色。 */
export const nodeStatusDot: Record<NodeStatus, string> = {
  pending: "bg-zinc-300 dark:bg-zinc-600",
  waiting: "bg-sky-500",
  running: "bg-brand-500",
  success: "bg-emerald-500",
  failed: "bg-red-500",
  skipped: "bg-zinc-400 dark:bg-zinc-500",
  cancelled: "bg-amber-500",
};

/** 运行状态 -> 文本视图里节点名的文字色（与画布状态色系一致）。 */
export const nodeStatusText: Record<NodeStatus, string> = {
  pending: "text-zinc-600 dark:text-zinc-300",
  waiting: "text-sky-600 dark:text-sky-400",
  running: "text-brand-600 dark:text-brand-400",
  success: "text-emerald-600 dark:text-emerald-400",
  failed: "text-red-600 dark:text-red-400",
  skipped: "text-zinc-400 dark:text-zinc-500",
  cancelled: "text-amber-600 dark:text-amber-400",
};

/** 连线上显示的简短条件文案（完整文案见 workflow.edge* i18n）。 */
export function edgeConditionLabel(
  condition: EdgeCondition,
  t: (key: string) => string,
): string {
  if (condition === "failure") return t("workflow.edgeLabel.failure");
  if (condition === "always") return t("workflow.edgeLabel.always");
  return t("workflow.edgeLabel.success");
}
