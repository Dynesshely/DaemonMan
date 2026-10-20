<script setup lang="ts">
import { computed } from "vue";
import { Ban, Boxes } from "lucide-vue-next";

import type { NodeStatus } from "@/api";
import { useI18n } from "@/modules/locales";
import { useWorkflowStore } from "@/modules/stores/workflows";

import { aggregateStatus } from "../layout";
import { nodeStatusDot } from "../meta";
import type { FlowNodeData } from "../types";

defineOptions({ inheritAttrs: false });

const props = defineProps<{
  id: string;
  data: FlowNodeData;
  selected?: boolean;
}>();

const store = useWorkflowStore();
const { t } = useI18n();

const node = computed(() => props.data.node);

const children = computed(
  () => store.draft?.nodes.filter((item) => item.parentId === props.id) ?? [],
);

/** 收集所有后代（含嵌套分组）的运行状态，用于分组聚合状态。 */
const aggregated = computed<NodeStatus>(() => {
  const draft = store.draft;
  if (!draft) return "pending";
  const statuses: NodeStatus[] = [];
  const visited = new Set<string>([props.id]);
  const walk = (parentId: string) => {
    for (const item of draft.nodes) {
      if (item.parentId !== parentId || visited.has(item.id)) continue;
      visited.add(item.id);
      if (item.kind === "group") {
        walk(item.id);
      } else {
        statuses.push(store.nodeRun(item.id)?.status ?? "pending");
      }
    }
  };
  walk(props.id);
  return aggregateStatus(statuses);
});

const disabled = computed(() => !node.value.enabled);

const containerClass = computed(() => {
  if (props.selected) {
    return "border-teal-500/80 bg-teal-500/[0.07] dark:border-teal-400/70 dark:bg-teal-400/[0.09] ring-2 ring-brand-500/50";
  }
  return "border-teal-400/50 bg-teal-500/[0.04] dark:border-teal-500/35 dark:bg-teal-400/[0.05]";
});

const headerClass = computed(() =>
  props.selected
    ? "border-teal-300 bg-teal-100/95 text-teal-900 dark:border-teal-400/50 dark:bg-teal-500/20 dark:text-teal-100"
    : "border-teal-200/70 bg-teal-50/90 text-teal-800 dark:border-teal-500/30 dark:bg-teal-500/10 dark:text-teal-200",
);

const parallelLabel = computed(() =>
  node.value.groupParallel
    ? t("workflow.text.parallelGroup")
    : t("workflow.text.sequentialGroup"),
);
</script>

<template>
  <div class="relative h-full w-full cursor-grab active:cursor-grabbing" :class="disabled ? 'opacity-60' : ''">
    <!-- 容器背景：可拖动整组（子节点层级更高，不会被子项遮挡） -->
    <div
      class="absolute inset-0 rounded-2xl border-2 border-dashed transition-colors"
      :class="containerClass"
    />

    <!-- 标题栏：可拖动整组 / 点击选中分组 -->
    <div
      class="pointer-events-auto absolute left-0 top-0 flex max-w-full cursor-grab items-center gap-1.5 overflow-hidden rounded-br-xl rounded-tl-2xl border-b border-r px-2.5 py-1.5 shadow-sm active:cursor-grabbing"
      :class="headerClass"
      :title="t('node.groupHint')"
    >
      <span
        class="flex h-4 w-4 shrink-0 items-center justify-center rounded bg-teal-500/15 text-teal-600 dark:text-teal-300"
      >
        <Boxes class="h-2.5 w-2.5" />
      </span>
      <span class="max-w-[200px] truncate text-[11px] font-semibold">
        {{ node.name || t("node.kind.group") }}
      </span>
      <span
        class="shrink-0 rounded-full bg-teal-500/15 px-1.5 py-0.5 text-[10px] font-medium text-teal-700 dark:bg-teal-400/20 dark:text-teal-200"
      >
        {{ parallelLabel }}
      </span>
      <span class="shrink-0 text-[10px] text-teal-600/80 dark:text-teal-300/80">
        {{ t("workflow.nodeCount", { count: children.length }) }}
      </span>
      <span
        v-if="aggregated !== 'pending'"
        class="h-1.5 w-1.5 shrink-0 rounded-full"
        :class="nodeStatusDot[aggregated]"
        :title="t(`status.${aggregated}`)"
      />
      <Ban
        v-if="disabled"
        class="h-3 w-3 shrink-0 text-teal-600/70 dark:text-teal-300/70"
        :title="t('common.disabled')"
      />
    </div>
  </div>
</template>
