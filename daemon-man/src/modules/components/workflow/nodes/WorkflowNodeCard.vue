<script setup lang="ts">
import { computed, inject } from "vue";
import { Handle, Position } from "@vue-flow/core";
import { Ban } from "lucide-vue-next";

import type { NodeStatus } from "@/api";
import { useI18n } from "@/modules/locales";
import { useIconStore } from "@/modules/stores/icons";
import { useWorkflowStore } from "@/modules/stores/workflows";
import { formatMs } from "@/modules/utils/format";

import { nodeKindMeta, nodeStatusBorder, nodeStatusDot } from "../meta";
import { WORKFLOW_STEPS, type FlowNodeData } from "../types";

defineOptions({ inheritAttrs: false });

const props = defineProps<{
  id: string;
  data: FlowNodeData;
  selected?: boolean;
}>();

const store = useWorkflowStore();
const icons = useIconStore();
const steps = inject(WORKFLOW_STEPS, null);
const { t } = useI18n();

const node = computed(() => props.data.node);
const meta = computed(() => nodeKindMeta[node.value.kind]);

/** 拓扑序执行步骤号（由画布 provide，分组节点没有）。 */
const step = computed<number | null>(() => steps?.value.get(props.id) ?? null);

/** 应用节点：优先显示 exe/.lnk 的真实图标（由视图层批量 ensure）。 */
const iconSrc = computed(() => {
  if (node.value.kind !== "application") return null;
  return icons.get(node.value.app.path);
});

/** draftRun.nodes[nodeId]，随运行事件实时更新。 */
const runState = computed(() => store.nodeRun(props.id));
const status = computed<NodeStatus | null>(() => runState.value?.status ?? null);

const disabled = computed(() => !node.value.enabled);

/** 第二行的关键摘要：路径 / 延时 / 进程名 / 命令。 */
const summary = computed(() => {
  const value = node.value;
  switch (value.kind) {
    case "application":
      return value.app.path.trim() || t("node.executableRequired");
    case "delay":
      return formatMs(value.delayMs);
    case "wait":
      return value.wait.processName.trim() || t("node.waitProcess");
    case "script":
      return value.script.command.trim() || t("node.scriptCommand");
    default:
      return "";
  }
});

const borderClass = computed(() =>
  status.value ? nodeStatusBorder[status.value] : nodeStatusBorder.pending,
);
</script>

<template>
  <div
    class="relative w-56 rounded-xl border-2 bg-white px-3 py-2.5 shadow-sm transition-colors dark:bg-zinc-900"
    :class="[
      borderClass,
      selected
        ? 'ring-2 ring-brand-500/60 ring-offset-2 ring-offset-white dark:ring-offset-zinc-950'
        : 'hover:border-brand-300 dark:hover:border-brand-500/60',
      disabled ? 'opacity-50' : '',
    ]"
  >
    <span
      v-if="status === 'running'"
      class="pointer-events-none absolute -inset-0.5 animate-pulse rounded-xl ring-2 ring-brand-500/50"
    />

    <div class="flex items-start gap-2">
      <span
        class="relative mt-px flex h-7 w-7 shrink-0 items-center justify-center rounded-lg"
        :class="meta.iconClass"
      >
        <img
          v-if="iconSrc"
          :src="iconSrc"
          alt=""
          class="h-4 w-4 rounded-sm object-contain"
        />
        <component :is="meta.icon" v-else class="h-4 w-4" />
        <span
          v-if="step"
          class="absolute -left-1.5 -top-1.5 rounded bg-brand-600 px-1 text-[9px] font-semibold leading-4 text-white shadow-sm"
          :title="t('workflow.execIndex', { index: step })"
        >
          #{{ step }}
        </span>
      </span>

      <div class="min-w-0 flex-1">
        <div class="flex items-center gap-1">
          <p
            class="truncate text-xs font-semibold text-zinc-800 dark:text-zinc-100"
          >
            {{ node.name || t(meta.labelKey) }}
          </p>
        </div>
        <p
          class="mt-0.5 truncate font-mono text-[10px] leading-4 text-zinc-500 dark:text-zinc-400"
          :title="summary"
        >
          {{ summary }}
        </p>
      </div>

      <div class="flex flex-col items-end gap-1">
        <span
          v-if="status"
          class="h-1.5 w-1.5 shrink-0 rounded-full"
          :class="[nodeStatusDot[status], status === 'running' ? 'animate-pulse' : '']"
          :title="t(`status.${status}`)"
        />
        <Ban
          v-if="disabled"
          class="h-3 w-3 shrink-0 text-zinc-400 dark:text-zinc-500"
          :title="t('common.disabled')"
        />
      </div>
    </div>

    <Handle
      type="target"
      :position="Position.Left"
      class="h-2.5! w-2.5! border-2! border-white! bg-brand-500! dark:border-zinc-900!"
    />
    <Handle
      type="source"
      :position="Position.Right"
      class="h-2.5! w-2.5! border-2! border-white! bg-brand-500! dark:border-zinc-900!"
    />
  </div>
</template>
