import { defineStore } from "pinia";
import { ref } from "vue";

import { api } from "@/api";
import type { ProcessInfo } from "@/api";

export const useProcessStore = defineStore("processes", () => {
  const processes = ref<ProcessInfo[]>([]);
  const loading = ref(false);
  const lastUpdated = ref<number | null>(null);

  async function load() {
    loading.value = true;
    try {
      processes.value = await api.listProcesses();
      lastUpdated.value = Date.now();
    } finally {
      loading.value = false;
    }
  }

  async function kill(pid: number) {
    await api.killProcess(pid);
    processes.value = processes.value.filter((item) => item.pid !== pid);
  }

  return { processes, loading, lastUpdated, load, kill };
});
