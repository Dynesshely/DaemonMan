<script setup lang="ts">
import { computed, inject } from "vue";
import { BaseEdge, EdgeLabelRenderer } from "@vue-flow/core";
import type { Position } from "@vue-flow/core";

import type { FlowEdgeData } from "../types";
import { WORKFLOW_EDGE_ROUTES, WORKFLOW_SELECTED_NODE } from "../types";
import { edgeConditionLabel } from "../meta";
import WorkflowDelayPill from "../nodes/WorkflowDelayPill.vue";
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

/**
 * 路由结果由画布统一计算（见 edgeRouting.ts），这里只负责取用：
 * 尚未算好时用一条简单的正交兜底路径，避免首帧空白。
 */
const routes = inject(WORKFLOW_EDGE_ROUTES, null);
const route = computed(() => routes?.value.get(props.id) ?? null);

const fallbackPath = computed(() => {
  const midX = Math.round(((props.sourceX + props.targetX) / 2) * 10) / 10;
  return `M ${props.sourceX} ${props.sourceY} L ${midX} ${props.sourceY} L ${midX} ${props.targetY} L ${props.targetX} ${props.targetY}`;
});

const path = computed(() => route.value?.path ?? fallbackPath.value);
const labelX = computed(
  () => route.value?.labelX ?? (props.sourceX + props.targetX) / 2,
);
const labelY = computed(
  () => route.value?.labelY ?? (props.sourceY + props.targetY) / 2,
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

/** 折叠了延时药丸的边：在路由算出的标签点渲染药丸。 */
const delayNodeId = computed(() => props.data?.delayNodeId ?? null);
const delayMs = computed(() => props.data?.delayMs ?? 0);

const selectedNodeId = inject(WORKFLOW_SELECTED_NODE, null);
const delaySelected = computed(
  () => Boolean(delayNodeId.value) && selectedNodeId?.value === delayNodeId.value,
);

const delayLabelStyle = computed(() => ({
  position: "absolute" as const,
  transform: `translate(-50%, -50%) translate(${labelX.value}px, ${labelY.value}px)`,
  pointerEvents: "none" as const,
}));
</script>

<template>
  <BaseEdge
    :id="id"
    :path="path"
    :marker-end="markerEnd"
    :style="pathStyle"
    :label="label"
    :label-x="labelX"
    :label-y="labelY"
    :label-style="labelStyle"
    :label-bg-style="labelBgStyle"
  />

  <EdgeLabelRenderer v-if="delayNodeId">
    <div class="nodrag nopan" :style="delayLabelStyle">
      <WorkflowDelayPill
        :delay-ms="delayMs"
        :selected="delaySelected || Boolean(selected)"
      />
    </div>
  </EdgeLabelRenderer>
</template>
