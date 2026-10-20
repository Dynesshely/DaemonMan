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
/** true = 执行流（相邻层 success），false = 依赖跳线。 */
const isFlow = computed(() => props.data?.flow ?? true);

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
  if (!isFlow.value) return theme.isDark ? "#71717a" : "#a1a1aa";
  return theme.isDark ? "#818cf8" : "#6366f1";
});

const strokeWidth = computed(() => {
  if (props.selected) return 2.5;
  return isFlow.value ? 2.2 : 1.5;
});

const pathStyle = computed(() => ({
  stroke: stroke.value,
  strokeWidth: strokeWidth.value,
  ...(isFlow.value ? {} : { strokeDasharray: "6 4" }),
}));

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
    :style="pathStyle"
    :label="label"
    :label-x="geometry[1]"
    :label-y="geometry[2]"
    :label-style="labelStyle"
    :label-bg-style="labelBgStyle"
  />
</template>
