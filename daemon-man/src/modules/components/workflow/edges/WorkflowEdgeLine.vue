<script setup lang="ts">
import { computed } from "vue";
import { BaseEdge, getSmoothStepPath } from "@vue-flow/core";
import type { Position } from "@vue-flow/core";

import type { FlowEdgeData } from "../types";
import { edgeConditionLabel } from "../meta";
import { useI18n } from "@/modules/locales";
import { useThemeStore } from "@/modules/stores/theme";

defineOptions({ inheritAttrs: false });

const props = defineProps<{
  id: string;
  sourceX: number;
  sourceY: number;
  targetX: number;
  targetY: number;
  sourcePosition: Position;
  targetPosition: Position;
  data: FlowEdgeData;
  markerEnd?: string;
  selected?: boolean;
}>();

const theme = useThemeStore();
const { t } = useI18n();

const condition = computed(() => props.data?.edge?.condition ?? "success");

const geometry = computed(() =>
  getSmoothStepPath({
    sourceX: props.sourceX,
    sourceY: props.sourceY,
    sourcePosition: props.sourcePosition,
    targetX: props.targetX,
    targetY: props.targetY,
    targetPosition: props.targetPosition,
    borderRadius: 18,
    offset: 22,
  }),
);

const stroke = computed(() => {
  if (props.selected) return theme.isDark ? "#a5b4fc" : "#4338ca";
  return theme.isDark ? "#818cf8" : "#6366f1";
});

const strokeWidth = computed(() => (props.selected ? 2.5 : 2));

/** 默认的 success 条件不显示标签，只有 failure / always 才标出来。 */
const label = computed(() =>
  condition.value === "success"
    ? undefined
    : edgeConditionLabel(condition.value, t),
);

const labelStyle = computed(() => ({
  fill: theme.isDark ? "#d4d4d8" : "#3f3f46",
  fontSize: "10px",
  fontWeight: 600,
}));

const labelBgStyle = computed(() => ({
  fill: theme.isDark ? "#18181b" : "#ffffff",
  stroke: theme.isDark ? "#3f3f46" : "#e4e4e7",
  strokeWidth: 1,
}));
</script>

<template>
  <BaseEdge
    :id="id"
    :path="geometry[0]"
    :marker-end="markerEnd"
    :style="{ stroke, strokeWidth }"
    :label="label"
    :label-x="geometry[1]"
    :label-y="geometry[2]"
    :label-style="labelStyle"
    :label-bg-style="labelBgStyle"
  />
</template>
