<script setup lang="ts">
import { computed } from "vue";
import { Monitor, Moon, Play, Sun } from "lucide-vue-next";

import BaseBadge from "@/modules/components/ui/BaseBadge.vue";
import BaseButton from "@/modules/components/ui/BaseButton.vue";
import { useI18n } from "@/modules/locales";
import { useThemeStore } from "@/modules/stores/theme";
import { useUiStore } from "@/modules/stores/ui";
import { useWorkflowStore } from "@/modules/stores/workflows";

const theme = useThemeStore();
const ui = useUiStore();
const workflowStore = useWorkflowStore();
const { t } = useI18n();

const themeIcon = computed(() =>
  theme.mode === "system" ? Monitor : theme.isDark ? Moon : Sun,
);

const themeLabel = computed(() =>
  theme.mode === "system"
    ? t("theme.system")
    : theme.isDark
      ? t("theme.dark")
      : t("theme.light"),
);

function cycleTheme() {
  const next =
    theme.mode === "system" ? "dark" : theme.mode === "dark" ? "light" : "system";
  theme.setMode(next);
}

async function runAll() {
  try {
    const runs = await workflowStore.runAll();
    if (runs.length === 0) {
      ui.notify("info", t("overview.noPipeline"));
    } else {
      ui.notify("success", `${t("overview.runAll")} · ${runs.length}`);
    }
  } catch (error) {
    ui.notify("error", t("workflow.runFailed") + "：" + String(error));
  }
}
</script>

<template>
  <header
    class="flex h-14 shrink-0 items-center justify-between gap-3 border-b border-zinc-200 bg-white/70 px-5 backdrop-blur dark:border-zinc-800 dark:bg-zinc-900/50"
  >
    <div class="flex min-w-0 items-center gap-2.5">
      <span
        class="truncate text-[11px] font-semibold uppercase tracking-[0.18em] text-zinc-400 dark:text-zinc-500"
      >
        DaemonMan
      </span>
      <BaseBadge
        v-if="workflowStore.runningCount > 0"
        variant="running"
        dot
        pulse
      >
        {{ t("status.running") }} · {{ workflowStore.runningCount }}
      </BaseBadge>
    </div>

    <div class="flex shrink-0 items-center gap-1.5">
      <BaseButton variant="ghost" size="sm" :title="t('overview.runAll')" @click="runAll">
        <Play class="h-4 w-4" />
        <span class="hidden sm:inline">{{ t("overview.runAll") }}</span>
      </BaseButton>
      <div class="mx-1 h-5 w-px bg-zinc-200 dark:bg-zinc-800" />
      <BaseButton
        variant="ghost"
        size="sm"
        icon-only
        :title="themeLabel + ' · ' + t('theme.toggle')"
        @click="cycleTheme"
      >
        <component :is="themeIcon" class="h-4 w-4" />
      </BaseButton>
    </div>
  </header>
</template>
