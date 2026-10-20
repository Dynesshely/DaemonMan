import { defineStore } from "pinia";
import { ref } from "vue";

import { api } from "@/api";

/** 后端 `get_file_icons` 单次批量上限。 */
const BATCH_SIZE = 40;
/** 请求的图标边长（px）。 */
const ICON_SIZE = 32;

/**
 * 文件图标缓存：`path -> PNG data URL | null`。
 * 提取失败也写入 null，避免对同一个不可用路径反复请求。
 */
export const useIconStore = defineStore("icons", () => {
  const icons = ref<Map<string, string | null>>(new Map());
  const pending = new Set<string>();

  function get(path: string | null | undefined): string | null {
    if (!path) return null;
    return icons.value.get(path) ?? null;
  }

  function isCached(path: string | null | undefined): boolean {
    if (!path) return false;
    return icons.value.has(path);
  }

  function setMany(entries: Map<string, string | null>) {
    if (entries.size === 0) return;
    const next = new Map(icons.value);
    for (const [path, dataUrl] of entries) {
      next.set(path, dataUrl);
    }
    icons.value = next;
  }

  /** 批量补齐缺失图标；失败静默（写入 null），不阻塞列表。 */
  async function ensure(paths: string[]) {
    const unique = [...new Set(paths.filter((path) => Boolean(path)))].filter(
      (path) => !icons.value.has(path) && !pending.has(path),
    );
    if (unique.length === 0) return;

    for (let index = 0; index < unique.length; index += BATCH_SIZE) {
      const batch = unique.slice(index, index + BATCH_SIZE);
      batch.forEach((path) => pending.add(path));
      try {
        const result = await api.getFileIcons(batch, ICON_SIZE);
        const resolvedByPath = new Map(
          result.map((entry) => [entry.path, entry.dataUrl]),
        );
        const resolved = new Map<string, string | null>();
        for (const path of batch) {
          resolved.set(path, resolvedByPath.get(path) ?? null);
        }
        setMany(resolved);
      } catch {
        const failed = new Map<string, string | null>();
        for (const path of batch) {
          failed.set(path, null);
        }
        setMany(failed);
      } finally {
        batch.forEach((path) => pending.delete(path));
      }
    }
  }

  return { icons, get, isCached, ensure };
});
