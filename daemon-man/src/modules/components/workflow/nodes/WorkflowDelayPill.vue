<script setup lang="ts">
import { computed } from "vue";
import { Clock } from "lucide-vue-next";

import { useI18n } from "@/modules/locales";
import { formatMs } from "@/modules/utils/format";

/**
 * 连线延时药丸（纯展示）：由边组件的 EdgeLabelRenderer 渲染在连线中点。
 * 点击事件交给下方的 edge interaction path，因此这里不接收指针事件。
 */
const props = defineProps<{
  delayMs: number;
  selected?: boolean;
}>();

const { t } = useI18n();

const value = computed(() => formatMs(props.delayMs));
</script>

<template>
  <div
    class="flex h-7 w-[88px] items-center justify-center gap-1 overflow-hidden rounded-full border px-2 text-[11px] font-medium shadow-sm transition-colors"
    :class="[
      'border-amber-300 bg-amber-50 text-amber-800 dark:border-amber-500/40 dark:bg-amber-500/15 dark:text-amber-200',
      selected ? 'ring-2 ring-brand-500/60' : '',
    ]"
    :title="t('workflow.edgeDelayValue', { ms: value })"
  >
    <Clock class="h-3 w-3 shrink-0" />
    <span class="truncate font-mono">{{ value }}</span>
  </div>
</template>
