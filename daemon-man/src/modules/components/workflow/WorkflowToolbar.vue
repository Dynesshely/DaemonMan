<script setup lang="ts">
import { computed } from "vue";
import { GitBranch, Rocket } from "lucide-vue-next";

import type { Workflow } from "@/api";
import BaseBadge from "@/modules/components/ui/BaseBadge.vue";
import ToggleSwitch from "@/modules/components/ui/ToggleSwitch.vue";
import { inputClass } from "@/modules/components/ui/form";
import { useI18n } from "@/modules/locales";
import { useWorkflowStore } from "@/modules/stores/workflows";
import { runStatusVariant } from "@/modules/utils/status";

const props = defineProps<{ draft: Workflow }>();

const store = useWorkflowStore();
const { t } = useI18n();

const run = computed(() => store.draftRun);

const progress = computed(() => {
  const nodes = run.value ? Object.values(run.value.nodes) : [];
  const finished = nodes.filter(
    (node) =>
      node.status === "success" ||
      node.status === "failed" ||
      node.status === "skipped" ||
      node.status === "cancelled",
  ).length;
  return { done: finished, total: nodes.length };
});
</script>

<template>
  <div
    class="flex shrink-0 flex-wrap items-center gap-2 rounded-xl border border-zinc-200 bg-white px-3 py-2 shadow-sm dark:border-zinc-800 dark:bg-zinc-900/60"
  >
    <input
      v-model="props.draft.name"
      :aria-label="t('workflow.name')"
      :placeholder="t('workflow.name')"
      :class="inputClass"
      class="w-56! shrink-0 py-1.5! text-sm! font-medium"
    />
    <input
      v-model="props.draft.description"
      :aria-label="t('workflow.desc')"
      :placeholder="t('workflow.desc')"
      :class="inputClass"
      class="min-w-0 flex-1 py-1.5! text-xs!"
    />

    <label
      class="flex shrink-0 cursor-pointer items-center gap-1.5 text-xs text-zinc-500 dark:text-zinc-400"
    >
      <ToggleSwitch
        :model-value="props.draft.enabled"
        size="sm"
        @update:model-value="props.draft.enabled = $event"
      />
      {{ t("workflow.enabledOnStartup") }}
    </label>

    <div class="flex shrink-0 items-center gap-1.5">
      <BaseBadge v-if="store.draftDirty" variant="warn" dot>
        {{ t("workflow.unsaved") }}
      </BaseBadge>

      <BaseBadge v-if="run" :variant="runStatusVariant(run.status)" dot :pulse="run.status === 'running'">
        {{ t(`status.${run.status}`) }}
      </BaseBadge>

      <BaseBadge v-if="run && progress.total > 0" variant="accent">
        {{ t("workflow.progress", progress) }}
      </BaseBadge>

      <span
        class="inline-flex items-center gap-1 text-[11px] text-zinc-400 dark:text-zinc-500"
      >
        <GitBranch class="h-3 w-3" />
        {{ t("workflow.nodeCount", { count: props.draft.nodes.length }) }}
      </span>
      <span
        class="inline-flex items-center gap-1 text-[11px] text-zinc-400 dark:text-zinc-500"
      >
        {{ t("workflow.edgeCount", { count: props.draft.edges.length }) }}
      </span>
      <Rocket
        v-if="props.draft.enabled"
        class="h-3.5 w-3.5 text-brand-500"
        :title="t('workflow.enabledOnStartup')"
      />
    </div>
  </div>
</template>
