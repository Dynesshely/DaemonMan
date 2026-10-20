/** 表单与卡片的统一 Tailwind 类名，避免各页面重复定义。 */

export const inputClass =
  "w-full rounded-lg border border-zinc-300 bg-white px-3 py-2 text-sm text-zinc-900 shadow-sm outline-none transition placeholder:text-zinc-400 focus:border-brand-500 focus:ring-2 focus:ring-brand-500/25 disabled:cursor-not-allowed disabled:opacity-60 dark:border-zinc-700 dark:bg-zinc-900 dark:text-zinc-100 dark:placeholder:text-zinc-500";

export const selectClass = `${inputClass} appearance-none pr-8`;

export const textareaClass = `${inputClass} min-h-[88px] resize-y font-mono text-xs leading-relaxed`;

export const labelClass =
  "text-[11px] font-semibold uppercase tracking-wider text-zinc-500 dark:text-zinc-400";

export const hintClass = "text-xs text-zinc-500 dark:text-zinc-400";

export const cardClass =
  "rounded-xl border border-zinc-200 bg-white shadow-sm dark:border-zinc-800 dark:bg-zinc-900/60";

export const cardHeaderClass =
  "flex items-start justify-between gap-3 border-b border-zinc-200/80 px-4 py-3 dark:border-zinc-800/80";

export const tableHeadClass =
  "text-left text-[11px] font-semibold uppercase tracking-wider text-zinc-500 dark:text-zinc-400";

export const tableCellClass =
  "py-2.5 pr-4 text-sm text-zinc-700 dark:text-zinc-300";

export const monoClass =
  "font-mono text-xs text-zinc-500 dark:text-zinc-400";
