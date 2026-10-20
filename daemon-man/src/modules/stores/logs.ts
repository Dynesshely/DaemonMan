import { defineStore } from "pinia";
import { ref } from "vue";

import { api } from "@/api";
import type { LogEntry } from "@/api";

const MAX_IN_MEMORY = 3000;

export const useLogStore = defineStore("logs", () => {
  const entries = ref<LogEntry[]>([]);
  const loading = ref(false);
  const loaded = ref(false);

  async function load(limit = 1000) {
    loading.value = true;
    try {
      entries.value = await api.getLogs(limit);
      loaded.value = true;
    } finally {
      loading.value = false;
    }
  }

  function push(entry: LogEntry) {
    entries.value.push(entry);
    if (entries.value.length > MAX_IN_MEMORY) {
      entries.value.splice(0, entries.value.length - MAX_IN_MEMORY);
    }
  }

  async function clear() {
    await api.clearLogs();
    entries.value = [];
  }

  async function exportTo(path: string) {
    await api.exportLogs(path);
  }

  return { entries, loading, loaded, load, push, clear, exportTo };
});
