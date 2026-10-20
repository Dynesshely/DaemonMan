import { defineStore } from "pinia";
import { computed, ref } from "vue";

import { api } from "@/api";
import type { AutostartItem, TakeOverResult } from "@/api";

export const useAutostartStore = defineStore("autostart", () => {
  const items = ref<AutostartItem[]>([]);
  const loading = ref(false);
  const scanning = ref(false);
  const error = ref<string | null>(null);
  const scannedAt = ref<number | null>(null);

  const enabledCount = computed(
    () => items.value.filter((item) => item.enabled).length,
  );
  const takenOverCount = computed(
    () => items.value.filter((item) => item.takenOver).length,
  );
  const disabledCount = computed(
    () => items.value.filter((item) => !item.enabled).length,
  );

  async function scan(force = false) {
    scanning.value = true;
    error.value = null;
    try {
      items.value = await api.scanAutostart(force);
      scannedAt.value = Date.now();
    } catch (err) {
      error.value = String(err);
    } finally {
      scanning.value = false;
      loading.value = false;
    }
  }

  function replaceItem(updated: AutostartItem) {
    const index = items.value.findIndex((item) => item.id === updated.id);
    if (index >= 0) {
      items.value[index] = updated;
    }
  }

  async function toggle(itemId: string, enabled: boolean) {
    const updated = await api.setAutostartEnabled(itemId, enabled);
    replaceItem(updated);
    return updated;
  }

  async function takeOver(
    itemIds: string[],
    options: { workflowId?: string; workflowName?: string } = {},
  ): Promise<TakeOverResult> {
    const result = await api.takeOverAutostart(
      itemIds,
      options.workflowId,
      options.workflowName,
    );
    await scan(true);
    return result;
  }

  async function release(itemId: string) {
    const updated = await api.releaseAutostart(itemId);
    replaceItem({ ...updated, takenOver: false });
    await scan(true);
    return updated;
  }

  return {
    items,
    loading,
    scanning,
    error,
    scannedAt,
    enabledCount,
    disabledCount,
    takenOverCount,
    scan,
    toggle,
    takeOver,
    release,
  };
});
