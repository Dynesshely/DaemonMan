/** 工作流编辑器内复用的面板样式与错误文本工具。 */

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
