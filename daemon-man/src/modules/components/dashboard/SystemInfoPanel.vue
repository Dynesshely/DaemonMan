<script setup lang="ts">
import { MonitorSmartphone, ShieldCheck, ShieldX } from "lucide-vue-next";

import type { SystemInfo } from "@/api";
import BaseBadge from "@/modules/components/ui/BaseBadge.vue";
import { useI18n } from "@/modules/locales";
import { formatUptime } from "@/modules/utils/format";

defineProps<{ info: SystemInfo | null }>();
const emit = defineEmits<{ openConfig: []; openLog: [] }>();

const { t } = useI18n();
</script>

<template>
  <div v-if="info" class="flex flex-col">
    <div
      class="flex items-center justify-between gap-3 border-b border-zinc-200/70 py-2 first:pt-0 dark:border-zinc-800/70"
    >
      <span class="text-xs text-zinc-500 dark:text-zinc-400">
        {{ t("overview.host") }}
      </span>
      <span
        class="flex min-w-0 items-center gap-1.5 truncate text-xs font-medium text-zinc-800 dark:text-zinc-200"
        :title="info.hostname"
      >
        <MonitorSmartphone class="h-3.5 w-3.5 shrink-0 text-zinc-400" />
        <span class="truncate">{{ info.hostname }}</span>
      </span>
    </div>

    <div
      class="flex items-center justify-between gap-3 border-b border-zinc-200/70 py-2 dark:border-zinc-800/70"
    >
      <span class="text-xs text-zinc-500 dark:text-zinc-400">
        {{ t("overview.os") }}
      </span>
      <span
        class="min-w-0 truncate text-xs font-medium text-zinc-800 dark:text-zinc-200"
        :title="`${info.os} · ${info.arch}`"
      >
        {{ info.os }} · {{ info.arch }}
      </span>
    </div>

    <div
      class="flex items-center justify-between gap-3 border-b border-zinc-200/70 py-2 dark:border-zinc-800/70"
    >
      <span class="text-xs text-zinc-500 dark:text-zinc-400">
        {{ t("overview.uptime") }}
      </span>
      <span class="tabular text-xs font-medium text-zinc-800 dark:text-zinc-200">
        {{ formatUptime(info.uptimeSecs) }}
      </span>
    </div>

    <div
      class="flex items-center justify-between gap-3 border-b border-zinc-200/70 py-2 dark:border-zinc-800/70"
    >
      <span class="text-xs text-zinc-500 dark:text-zinc-400">
        {{ t("overview.privilege") }}
      </span>
      <BaseBadge :variant="info.elevated ? 'success' : 'neutral'">
        <ShieldCheck v-if="info.elevated" class="h-3 w-3" />
        <ShieldX v-else class="h-3 w-3" />
        {{
          info.elevated
            ? t("overview.elevated")
            : t("overview.notElevated")
        }}
      </BaseBadge>
    </div>

    <div
      class="flex items-center justify-between gap-3 border-b border-zinc-200/70 py-2 dark:border-zinc-800/70"
    >
      <span class="text-xs text-zinc-500 dark:text-zinc-400">
        {{ t("overview.version") }}
      </span>
      <span class="tabular text-xs font-medium text-zinc-800 dark:text-zinc-200">
        v{{ info.appVersion }}
      </span>
    </div>

    <div class="flex items-center justify-between gap-3 py-2">
      <span class="shrink-0 text-xs text-zinc-500 dark:text-zinc-400">
        {{ t("overview.configPath") }}
      </span>
      <button
        type="button"
        class="min-w-0 truncate rounded px-1 py-0.5 font-mono text-[11px] text-brand-600 transition hover:bg-brand-50 hover:text-brand-700 dark:text-brand-400 dark:hover:bg-brand-500/10 dark:hover:text-brand-300"
        :title="info.configPath"
        @click="emit('openConfig')"
      >
        {{ info.configPath }}
      </button>
    </div>

    <div class="flex items-center justify-between gap-3 py-2">
      <span class="shrink-0 text-xs text-zinc-500 dark:text-zinc-400">
        {{ t("overview.logPath") }}
      </span>
      <button
        type="button"
        class="min-w-0 truncate rounded px-1 py-0.5 font-mono text-[11px] text-brand-600 transition hover:bg-brand-50 hover:text-brand-700 dark:text-brand-400 dark:hover:bg-brand-500/10 dark:hover:text-brand-300"
        :title="info.logPath"
        @click="emit('openLog')"
      >
        {{ info.logPath }}
      </button>
    </div>
  </div>

  <div v-else class="flex flex-col gap-2 py-2">
    <div class="h-3 w-2/3 animate-pulse rounded bg-zinc-200 dark:bg-zinc-800" />
    <div class="h-3 w-1/2 animate-pulse rounded bg-zinc-200 dark:bg-zinc-800" />
    <div class="h-3 w-3/4 animate-pulse rounded bg-zinc-200 dark:bg-zinc-800" />
  </div>
</template>
