<script setup lang="ts">
import { computed } from "vue";
import { FolderCog, ShieldCheck } from "lucide-vue-next";

import { api } from "@/api";
import { useI18n } from "@/modules/locales";
import { useAppStore } from "@/modules/stores/app";
import { useUiStore } from "@/modules/stores/ui";
import { useWorkflowStore } from "@/modules/stores/workflows";
import { formatUptime } from "@/modules/utils/format";

const appStore = useAppStore();
const workflowStore = useWorkflowStore();
const ui = useUiStore();
const { t } = useI18n();

const version = computed(() => appStore.systemInfo?.appVersion ?? "0.1.0");
const uptime = computed(() =>
  formatUptime(appStore.systemInfo?.uptimeSecs ?? 0),
);
const elevated = computed(() => appStore.systemInfo?.elevated ?? false);

const configPath = computed(() => {
  const raw = appStore.systemInfo?.configPath ?? "";
  return raw.replace(/^[A-Za-z]:\\Users\\[^\\]+/, "~");
});

async function openConfigDir() {
  try {
    await api.openConfigDir();
  } catch (error) {
    ui.notify("error", String(error));
  }
}
</script>

<template>
  <footer
    class="flex h-8 shrink-0 items-center justify-between gap-3 border-t border-zinc-200 bg-white/60 px-4 text-[11px] text-zinc-500 dark:border-zinc-800 dark:bg-zinc-900/40 dark:text-zinc-500"
  >
    <div class="flex min-w-0 items-center gap-3">
      <span class="shrink-0 font-medium">DaemonMan v{{ version }}</span>
      <span class="hidden shrink-0 items-center gap-1 md:inline-flex">
        {{ t("overview.uptime") }} {{ uptime }}
      </span>
      <span
        v-if="workflowStore.runningCount > 0"
        class="inline-flex shrink-0 items-center gap-1 text-brand-600 dark:text-brand-400"
      >
        <span class="h-1.5 w-1.5 animate-pulse rounded-full bg-brand-500" />
        {{ workflowStore.runningCount }} {{ t("status.running") }}
      </span>
      <span
        v-if="elevated"
        class="hidden shrink-0 items-center gap-1 text-emerald-600 dark:text-emerald-400 sm:inline-flex"
      >
        <ShieldCheck class="h-3 w-3" />
        {{ t("overview.elevated") }}
      </span>
    </div>

    <button
      type="button"
      class="inline-flex min-w-0 items-center gap-1.5 rounded px-1.5 py-0.5 transition hover:bg-zinc-200/60 hover:text-zinc-700 dark:hover:bg-zinc-800 dark:hover:text-zinc-300"
      :title="appStore.systemInfo?.configPath"
      @click="openConfigDir"
    >
      <FolderCog class="h-3 w-3 shrink-0" />
      <span class="truncate">{{ configPath }}</span>
    </button>
  </footer>
</template>
