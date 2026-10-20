<script setup lang="ts">
import { computed } from "vue";
import { ArrowRight, Trash2 } from "lucide-vue-next";

import type { EdgeCondition, WorkflowEdge } from "@/api";
import BaseButton from "@/modules/components/ui/BaseButton.vue";
import { useI18n } from "@/modules/locales";
import { useWorkflowStore } from "@/modules/stores/workflows";

import { nodeKindMeta } from "./meta";
import { fieldGroupClass, sectionLabelClass } from "./shared";

const props = defineProps<{ edge: WorkflowEdge }>();
const emit = defineEmits<{ delete: [] }>();

const store = useWorkflowStore();
const { t } = useI18n();

function nodeName(id: string): string {
  const node = store.draft?.nodes.find((item) => item.id === id);
  if (!node) return "—";
  return node.name || t(nodeKindMeta[node.kind].labelKey);
}

const sourceName = computed(() => nodeName(props.edge.source));
const targetName = computed(() => nodeName(props.edge.target));

const conditionOptions: {
  value: EdgeCondition;
  labelKey: string;
  dotClass: string;
}[] = [
  {
    value: "success",
    labelKey: "workflow.edgeSuccess",
    dotClass: "bg-emerald-500",
  },
  {
    value: "failure",
    labelKey: "workflow.edgeFailure",
    dotClass: "bg-red-500",
  },
  {
    value: "always",
    labelKey: "workflow.edgeAlways",
    dotClass: "bg-brand-500",
  },
];
</script>

<template>
  <div class="space-y-4 p-4">
    <div class="flex items-start justify-between gap-2">
      <div class="min-w-0">
        <p class="text-xs font-semibold text-zinc-800 dark:text-zinc-100">
          {{ t("workflow.edgeCondition") }}
        </p>
        <p class="truncate font-mono text-[10px] text-zinc-400 dark:text-zinc-500">
          ID {{ props.edge.id.slice(0, 16) }}
        </p>
      </div>
      <BaseButton
        variant="ghost"
        size="sm"
        icon-only
        :title="t('workflow.deleteEdge')"
        @click="emit('delete')"
      >
        <Trash2 class="h-4 w-4 text-red-500" />
      </BaseButton>
    </div>

    <div
      :class="fieldGroupClass"
      class="flex items-center gap-1.5 text-xs text-zinc-600 dark:text-zinc-300"
    >
      <span class="truncate font-medium">{{ sourceName }}</span>
      <ArrowRight class="h-3.5 w-3.5 shrink-0 text-zinc-400" />
      <span class="truncate font-medium">{{ targetName }}</span>
    </div>

    <div class="space-y-1.5">
      <p :class="sectionLabelClass">{{ t("workflow.edgeCondition") }}</p>
      <button
        v-for="option in conditionOptions"
        :key="option.value"
        type="button"
        class="flex w-full items-center gap-2 rounded-lg border px-3 py-2 text-left transition"
        :class="
          props.edge.condition === option.value
            ? 'border-brand-400/80 bg-brand-50/70 dark:border-brand-500/60 dark:bg-brand-500/10'
            : 'border-zinc-200 hover:bg-zinc-50 dark:border-zinc-800 dark:hover:bg-zinc-800/60'
        "
        @click="props.edge.condition = option.value"
      >
        <span class="h-2 w-2 shrink-0 rounded-full" :class="option.dotClass" />
        <span class="min-w-0 flex-1 text-xs text-zinc-700 dark:text-zinc-200">
          {{ t(option.labelKey) }}
        </span>
      </button>
    </div>

    <BaseButton variant="danger" size="sm" block @click="emit('delete')">
      <Trash2 class="h-3.5 w-3.5" />
      {{ t("workflow.deleteEdge") }}
    </BaseButton>
  </div>
</template>
