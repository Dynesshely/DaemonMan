<script setup lang="ts">
import type { NodeStatus } from "@/api";
import BaseBadge from "@/modules/components/ui/BaseBadge.vue";
import { useI18n } from "@/modules/locales";
import { nodeStatusVariant } from "@/modules/utils/status";

/**
 * 节点运行状态徽标。
 * BaseBadge 没有 teal 变体，这里单独处理「已运行跳过（skipped-existing）」。
 */
defineProps<{ status: NodeStatus }>();

const { t } = useI18n();
</script>

<template>
  <span
    v-if="status === 'skipped-existing'"
    class="inline-flex items-center gap-1.5 rounded-full bg-teal-50 px-2 py-0.5 text-[11px] font-medium text-teal-700 ring-1 ring-inset ring-teal-200 dark:bg-teal-500/15 dark:text-teal-300 dark:ring-teal-500/30"
  >
    <span class="h-1.5 w-1.5 shrink-0 rounded-full bg-teal-500" />
    {{ t("status.skipped-existing") }}
  </span>
  <BaseBadge
    v-else
    :variant="nodeStatusVariant(status)"
    dot
    :pulse="status === 'running'"
  >
    {{ t(`status.${status}`) }}
  </BaseBadge>
</template>
