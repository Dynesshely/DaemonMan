<script setup lang="ts">
import { computed } from "vue";
import { Handle, Position } from "@vue-flow/core";
import { Clock } from "lucide-vue-next";

import { useI18n } from "@/modules/locales";
import { formatMs } from "@/modules/utils/format";

import type { FlowNodeData } from "../types";

defineOptions({ inheritAttrs: false });

const props = defineProps<{
  id: string;
  data: FlowNodeData;
  selected?: boolean;
}>();

const { t } = useI18n();

const node = computed(() => props.data.node);
const value = computed(() => formatMs(node.value.delayMs));

/** 隐藏手柄（1px 透明 + 不接收指针），让 source→延时→target 看起来是一条连续线。 */
const handleClass =
  "h-1! w-1! border-0! bg-transparent! opacity-0! pointer-events-none!";
</script>

<template>
  <div
    class="relative flex h-full w-full items-center justify-center gap-1 overflow-hidden rounded-full border px-2 text-[11px] font-medium shadow-sm transition-colors"
    :class="[
      node.enabled
        ? 'border-amber-300 bg-amber-50 text-amber-800 dark:border-amber-500/40 dark:bg-amber-500/15 dark:text-amber-200'
        : 'border-zinc-300 bg-zinc-100 text-zinc-400 opacity-60 dark:border-zinc-700 dark:bg-zinc-800 dark:text-zinc-500',
      selected ? 'ring-2 ring-brand-500/60' : '',
    ]"
    :title="t('workflow.edgeDelayValue', { ms: value })"
  >
    <Clock class="h-3 w-3 shrink-0" />
    <span class="truncate font-mono">{{ value }}</span>

    <Handle
      type="target"
      :position="Position.Left"
      :class="handleClass"
    />
    <Handle
      type="source"
      :position="Position.Right"
      :class="handleClass"
    />
  </div>
</template>
