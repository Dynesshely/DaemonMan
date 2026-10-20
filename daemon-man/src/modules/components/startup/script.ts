import type { AutostartItem } from "@/api";

/** 右键「编辑启动内容」允许的文本类脚本扩展名。 */
export const EDITABLE_SCRIPT_EXTENSIONS = [
  "bat",
  "cmd",
  "ps1",
  "vbs",
  "vbe",
  "js",
  "jse",
  "wsf",
  "reg",
  "txt",
  "ini",
  "sh",
  "py",
] as const;

function hasEditableExtension(path: string | null | undefined): boolean {
  if (!path) return false;
  const lower = path.toLowerCase();
  return EDITABLE_SCRIPT_EXTENSIONS.some((extension) =>
    lower.endsWith(`.${extension}`),
  );
}

/**
 * 是否可在内置编辑器中编辑启动内容：
 * `executable` 命中白名单，或启动文件夹项的 `locator` 命中白名单。
 * 「备份后移除」的项原文件已不在原位，不可编辑。
 */
export function canEditScript(item: AutostartItem | null): boolean {
  if (!item || item.removed) return false;
  if (hasEditableExtension(item.executable)) return true;
  if (item.source === "startup-folder") {
    return hasEditableExtension(item.locator);
  }
  return false;
}

/** 是否能在资源管理器中定位（有可执行文件路径，或启动文件夹项）。 */
export function canRevealItem(item: AutostartItem | null): boolean {
  if (!item || item.removed) return false;
  return Boolean(item.executable) || item.source === "startup-folder";
}
