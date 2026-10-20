<script setup lang="ts">
import { onMounted, onUnmounted } from "vue";
import { useRouter } from "vue-router";
import type { UnlistenFn } from "@tauri-apps/api/event";

import AppShell from "@/modules/components/layout/AppShell.vue";
import { events } from "@/api";
import { useAppStore } from "@/modules/stores/app";
import { useWorkflowStore } from "@/modules/stores/workflows";
import { useLogStore } from "@/modules/stores/logs";
import { useUiStore } from "@/modules/stores/ui";

const router = useRouter();
const appStore = useAppStore();
const workflowStore = useWorkflowStore();
const logStore = useLogStore();
const ui = useUiStore();

let unlisteners: UnlistenFn[] = [];
let systemInfoTimer: number | undefined;

onMounted(async () => {
  try {
    await appStore.init();
    await Promise.all([
      workflowStore.load(),
      workflowStore.loadRuns(),
      logStore.load(),
    ]);
  } catch (error) {
    ui.notify("error", String(error));
  }

  unlisteners.push(
    await events.onRun((run) => workflowStore.applyRun(run)),
    await events.onLog((entry) => logStore.push(entry)),
    await events.onNavigate((path) => {
      const target = path.startsWith("/") ? path : `/${path}`;
      void router.push(target);
    }),
  );

  // 定时刷新系统信息（运行时长、权限、自启动计数），不轮询进程列表。
  systemInfoTimer = window.setInterval(() => {
    void appStore.refreshSystemInfo();
  }, 30_000);
});

onUnmounted(() => {
  unlisteners.forEach((fn) => fn());
  unlisteners = [];
  if (systemInfoTimer !== undefined) {
    window.clearInterval(systemInfoTimer);
  }
});
</script>

<template>
  <AppShell />
</template>
