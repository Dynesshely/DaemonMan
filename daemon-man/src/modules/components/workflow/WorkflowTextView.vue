<script setup lang="ts">
import { computed, ref } from "vue";
import {
  ChevronDown,
  ChevronRight,
  Layers,
  ListTree,
  Workflow,
} from "lucide-vue-next";

import type {
  NodeStatus,
  Workflow as WorkflowModel,
  WorkflowNode,
} from "@/api";
import BaseBadge from "@/modules/components/ui/BaseBadge.vue";
import EmptyState from "@/modules/components/ui/EmptyState.vue";
import { useI18n } from "@/modules/locales";
import { useIconStore } from "@/modules/stores/icons";
import { useWorkflowStore } from "@/modules/stores/workflows";

import { buildAdjacency, computeTopology } from "./graph";
import { aggregateStatus, buildGroupChildren } from "./layout";
import { buildDelayLayoutGraph, computeDelayPills } from "./delayPill";
import {
  edgeConditionLabel,
  nodeKindMeta,
  nodeStatusDot,
  nodeStatusText,
} from "./meta";
import WorkflowStatusBadge from "./WorkflowStatusBadge.vue";

const props = defineProps<{
  draft: WorkflowModel | null;
  selectedNodeId: string | null;
}>();

const emit = defineEmits<{ select: [nodeId: string] }>();

const store = useWorkflowStore();
const icons = useIconStore();
const { t } = useI18n();

const nodes = computed<WorkflowNode[]>(() => props.draft?.nodes ?? []);
const edges = computed(() => props.draft?.edges ?? []);

const adjacency = computed(() => buildAdjacency(nodes.value, edges.value));
const topology = computed(() => computeTopology(nodes.value, edges.value));
const layerInfo = topology;
const groupIndex = computed(() => buildGroupChildren(nodes.value));
/** 折叠到连线上的延时节点（行内加「连线延时」标记）。 */
const edgeDelayPills = computed(() =>
  computeDelayPills(nodes.value, edges.value),
);
/** 可见节点连续步骤号：折叠药丸后重新编号（与画布节点徽标一致）。 */
const stepTopology = computed(() => {
  const pills = edgeDelayPills.value;
  if (pills.size === 0) return topology.value;
  const graph = buildDelayLayoutGraph(nodes.value, edges.value, pills);
  return computeTopology(graph.nodes, graph.edges);
});
const isEmpty = computed(() => nodes.value.length === 0);

function stepOf(nodeId: string): number | null {
  return stepTopology.value.stepOf.get(nodeId) ?? null;
}

/** 应用节点的真实 exe/.lnk 图标（视图层已批量 ensure）。 */
function iconOf(node: WorkflowNode): string | null {
  if (node.kind !== "application") return null;
  return icons.get(node.app.path);
}

function iconSrc(node: WorkflowNode): string {
  return iconOf(node) ?? "";
}

/** 折叠的分组 id（默认全部展开）。 */
const collapsedGroups = ref<Set<string>>(new Set());

function toggleCollapsed(groupId: string) {
  const next = new Set(collapsedGroups.value);
  if (next.has(groupId)) next.delete(groupId);
  else next.add(groupId);
  collapsedGroups.value = next;
}

function nameOf(nodeId: string): string {
  const node = nodes.value.find((item) => item.id === nodeId);
  if (!node) return "—";
  return node.name || t(nodeKindMeta[node.kind].labelKey);
}

function statusOf(nodeId: string) {
  return store.nodeRun(nodeId)?.status ?? "pending";
}

/** 分组聚合状态：递归收集所有后代叶子节点的运行状态。 */
function groupStatus(groupId: string) {
  const statuses: NodeStatus[] = [];
  const visited = new Set<string>([groupId]);
  const walk = (parentId: string) => {
    for (const node of nodes.value) {
      if (node.parentId !== parentId || visited.has(node.id)) continue;
      visited.add(node.id);
      if (node.kind === "group") walk(node.id);
      else statuses.push(statusOf(node.id));
    }
  };
  walk(groupId);
  return aggregateStatus(statuses);
}

function groupLayer(group: WorkflowNode, stack = new Set<string>()): number {
  if (stack.has(group.id)) return 1;
  stack.add(group.id);
  let layer = Number.POSITIVE_INFINITY;
  for (const node of nodes.value) {
    if (node.parentId !== group.id) continue;
    const value =
      node.kind === "group"
        ? groupLayer(node, stack)
        : layerInfo.value.layerOf.get(node.id) ?? 1;
    layer = Math.min(layer, value);
  }
  stack.delete(group.id);
  return Number.isFinite(layer) ? layer : 1;
}

function layerOf(node: WorkflowNode): number {
  if (node.kind === "group") return groupLayer(node);
  return layerInfo.value.layerOf.get(node.id) ?? 1;
}

/** 分组的外部上游依赖（分组内节点之间的依赖不计入）。 */
function externalUpstreamOf(groupId: string) {
  const inside = new Set<string>([groupId]);
  const collect = (parentId: string) => {
    for (const node of nodes.value) {
      if (node.parentId !== parentId || inside.has(node.id)) continue;
      inside.add(node.id);
      if (node.kind === "group") collect(node.id);
    }
  };
  collect(groupId);

  const seen = new Set<string>();
  const result: { key: string; name: string; condition: string }[] = [];
  for (const edge of edges.value) {
    if (!inside.has(edge.target) || inside.has(edge.source)) continue;
    const key = `${edge.source}:${edge.condition}`;
    if (seen.has(key)) continue;
    seen.add(key);
    result.push({
      key,
      name: nameOf(edge.source),
      condition: edgeConditionLabel(edge.condition, t),
    });
  }
  return result;
}

function upstreamOf(nodeId: string) {
  return (adjacency.value.incoming.get(nodeId) ?? []).map((edge) => ({
    id: edge.source,
    name: nameOf(edge.source),
    condition: edgeConditionLabel(edge.condition, t),
  }));
}

function downstreamCount(nodeId: string): number {
  return adjacency.value.outgoing.get(nodeId)?.length ?? 0;
}

/** 目标 / 参数列的紧凑文本。 */
function targetText(node: WorkflowNode): string {
  switch (node.kind) {
    case "application": {
      const args = node.app.args.length > 0 ? ` ${node.app.args.join(" ")}` : "";
      return `${node.app.path || "—"}${args}`;
    }
    case "delay":
      return `${node.delayMs} ms`;
    case "wait":
      return `${node.wait.processName || "—"} · ${node.wait.timeoutMs} ms`;
    case "script":
      return node.script.command || "—";
    default:
      return "—";
  }
}

interface TreeRow {
  node: WorkflowNode;
  depth: number;
  isGroup: boolean;
  childCount: number;
  prefix: string;
  collapsed: boolean;
}

/** 以 parentId 展开的树形行；顶层按执行层排序，同级保持 draft 顺序。 */
const treeRows = computed<TreeRow[]>(() => {
  const { childrenOf, groupIds } = groupIndex.value;
  const draftIndex = new Map(nodes.value.map((node, index) => [node.id, index]));
  const rows: TreeRow[] = [];
  const visited = new Set<string>();

  const topLevel = nodes.value.filter((node) => {
    const parentId = node.parentId;
    return !parentId || parentId === node.id || !groupIds.has(parentId);
  });
  const stepKey = (node: WorkflowNode) =>
    topology.value.stepOf.get(node.id) ?? Number.MAX_SAFE_INTEGER;
  const byTopology = (left: WorkflowNode, right: WorkflowNode) =>
    stepKey(left) - stepKey(right) ||
    (draftIndex.get(left.id) ?? 0) - (draftIndex.get(right.id) ?? 0);

  topLevel.sort(
    (left, right) => layerOf(left) - layerOf(right) || byTopology(left, right),
  );

  const walk = (node: WorkflowNode, depth: number, prefix: string) => {
    if (visited.has(node.id)) return;
    visited.add(node.id);
    const children =
      node.kind === "group"
        ? [...(childrenOf.get(node.id) ?? [])].sort(byTopology)
        : [];
    const collapsed = node.kind === "group" && collapsedGroups.value.has(node.id);
    rows.push({
      node,
      depth,
      isGroup: node.kind === "group",
      childCount: children.length,
      prefix,
      collapsed,
    });
    if (collapsed) return;
    children.forEach((child, index) => {
      walk(child, depth + 1, index === children.length - 1 ? "└" : "├");
    });
  };

  topLevel.forEach((node) => walk(node, 0, ""));

  // 兜底：parentId 成环等异常结构里没被走到的节点也不能丢。
  for (const node of nodes.value) {
    if (!visited.has(node.id)) walk(node, 0, "");
  }
  return rows;
});

const summaryText = computed(() =>
  t("workflow.text.summary", {
    nodes: nodes.value.length,
    edges: edges.value.length,
    layers: layerInfo.value.count,
  }),
);

function rowStatus(row: TreeRow) {
  return row.isGroup ? groupStatus(row.node.id) : statusOf(row.node.id);
}

function indentClass(depth: number): string {
  return (
    ["pl-0", "pl-5", "pl-10", "pl-14", "pl-20"][Math.min(depth, 4)] ?? "pl-20"
  );
}
</script>

<template>
  <div class="flex h-full min-h-0 flex-col bg-white dark:bg-zinc-900/40">
    <!-- 摘要条 -->
    <div
      class="flex shrink-0 flex-wrap items-center gap-x-3 gap-y-1 border-b border-zinc-200 px-3 py-2 dark:border-zinc-800"
    >
      <span
        class="inline-flex items-center gap-1.5 text-[11px] font-semibold uppercase tracking-wider text-zinc-500 dark:text-zinc-400"
      >
        <ListTree class="h-3.5 w-3.5" />
        {{ t("workflow.view.text") }}
      </span>
      <span class="text-[11px] text-zinc-500 dark:text-zinc-400">
        {{ summaryText }}
      </span>
      <BaseBadge v-if="props.draft?.skipExistingProcesses" variant="info">
        {{ t("workflow.skipExisting") }}
      </BaseBadge>
    </div>

    <!-- 节点清单（树形） -->
    <div class="flex min-h-0 flex-1 flex-col">
      <div class="flex shrink-0 items-center justify-between px-3 pb-1 pt-2">
        <p
          class="text-[10px] font-semibold uppercase tracking-wider text-zinc-400 dark:text-zinc-500"
        >
          {{ t("workflow.text.nodes") }}
        </p>
        <span class="tabular text-[10px] text-zinc-400 dark:text-zinc-500">
          {{ treeRows.length }}
        </span>
      </div>

      <div v-if="isEmpty" class="flex min-h-0 flex-1 items-center justify-center p-6">
        <EmptyState
          :title="t('workflow.text.noNodes')"
          :description="t('workflow.text.emptyHint')"
          compact
        >
          <template #icon>
            <Workflow class="h-5 w-5" />
          </template>
        </EmptyState>
      </div>

      <div v-else class="min-h-0 flex-1 overflow-auto">
        <div class="min-w-[760px]">
          <!-- 表头 -->
          <div
            class="sticky top-0 z-10 grid h-7 grid-cols-[minmax(190px,1.5fr)_56px_84px_minmax(150px,1fr)_168px_48px_92px] items-center border-b border-zinc-200 bg-zinc-50 px-2 text-[10px] font-semibold uppercase tracking-wider text-zinc-400 dark:border-zinc-800 dark:bg-zinc-900 dark:text-zinc-500"
          >
            <span class="px-1">{{ t("workflow.text.name") }}</span>
            <span class="px-1 text-center">{{ t("workflow.text.layer") }}</span>
            <span class="px-1">{{ t("workflow.text.type") }}</span>
            <span class="px-1">{{ t("workflow.text.target") }}</span>
            <span class="px-1">{{ t("workflow.text.dependencies") }}</span>
            <span class="px-1 text-center">{{ t("workflow.text.downstream") }}</span>
            <span class="px-1">{{ t("workflow.text.status") }}</span>
          </div>

          <!-- 行 -->
          <div
            v-for="row in treeRows"
            :key="row.node.id"
            class="grid h-7 cursor-pointer grid-cols-[minmax(190px,1.5fr)_56px_84px_minmax(150px,1fr)_168px_48px_92px] items-center border-b border-zinc-100 px-2 text-xs transition-colors dark:border-zinc-800/60"
            :class="[
              row.node.id === props.selectedNodeId
                ? 'bg-brand-50 dark:bg-brand-500/10'
                : 'hover:bg-zinc-50 dark:hover:bg-zinc-800/50',
              row.node.enabled ? '' : 'opacity-50',
            ]"
            :tabindex="0"
            @click="emit('select', row.node.id)"
            @keydown.enter.prevent="emit('select', row.node.id)"
          >
            <!-- 名称（树形缩进 + 展开标记 + 状态点 + 类型图标） -->
            <div
              class="flex min-w-0 items-center gap-1 px-1"
              :class="indentClass(row.depth)"
            >
              <span
                v-if="row.depth > 0"
                class="w-3 shrink-0 font-mono text-[11px] leading-none text-zinc-300 dark:text-zinc-600"
              >
                {{ row.prefix }}
              </span>
              <button
                v-if="row.isGroup && row.childCount > 0"
                type="button"
                class="flex h-4 w-4 shrink-0 items-center justify-center rounded text-zinc-400 transition hover:bg-zinc-200/70 hover:text-zinc-600 dark:hover:bg-zinc-700/70 dark:hover:text-zinc-200"
                :title="row.node.name"
                @click.stop="toggleCollapsed(row.node.id)"
              >
                <ChevronRight v-if="row.collapsed" class="h-3 w-3" />
                <ChevronDown v-else class="h-3 w-3" />
              </button>
              <span v-else-if="row.isGroup" class="w-4 shrink-0" />

              <span
                class="h-1.5 w-1.5 shrink-0 rounded-full"
                :class="nodeStatusDot[rowStatus(row)]"
              />
              <span
                class="flex h-4 w-4 shrink-0 items-center justify-center rounded"
                :class="nodeKindMeta[row.node.kind].iconClass"
              >
                <img
                  v-if="iconOf(row.node)"
                  :src="iconSrc(row.node)"
                  alt=""
                  class="h-3.5 w-3.5 rounded-sm object-contain"
                />
                <component
                  v-else
                  :is="nodeKindMeta[row.node.kind].icon"
                  class="h-2.5 w-2.5"
                />
              </span>
              <span
                class="truncate font-medium"
                :class="nodeStatusText[rowStatus(row)]"
                :title="nameOf(row.node.id)"
              >
                {{ nameOf(row.node.id) }}
              </span>
              <span
                v-if="!row.isGroup && edgeDelayPills.has(row.node.id)"
                class="shrink-0 rounded-full bg-amber-500/15 px-1.5 text-[10px] font-medium text-amber-700 dark:bg-amber-400/20 dark:text-amber-200"
                :title="t('workflow.edgeDelayHint')"
              >
                {{ t("workflow.edgeDelay") }}
              </span>
              <template v-if="row.isGroup">
                <span
                  class="shrink-0 rounded-full bg-teal-500/15 px-1.5 text-[10px] font-medium text-teal-700 dark:bg-teal-400/20 dark:text-teal-200"
                >
                  {{
                    row.node.groupParallel
                      ? t("workflow.text.parallelGroup")
                      : t("workflow.text.sequentialGroup")
                  }}
                </span>
                <span
                  class="shrink-0 text-[10px] text-zinc-400 dark:text-zinc-500"
                >
                  {{ t("workflow.nodeCount", { count: row.childCount }) }}
                </span>
              </template>
            </div>

            <!-- 层 + 执行步骤号 -->
            <div class="flex items-center justify-center gap-1 px-1">
              <span
                v-if="stepOf(row.node.id)"
                class="shrink-0 font-mono text-[10px] font-semibold text-brand-600 dark:text-brand-400"
                :title="t('workflow.execIndex', { index: stepOf(row.node.id) ?? 0 })"
              >
                #{{ stepOf(row.node.id) }}
              </span>
              <span
                class="inline-flex h-4 min-w-4 items-center justify-center rounded bg-zinc-100 px-1 text-[10px] font-medium text-zinc-500 dark:bg-zinc-800 dark:text-zinc-400"
              >
                {{ layerOf(row.node) }}
              </span>
            </div>

            <!-- 类型 -->
            <div class="truncate px-1 text-zinc-600 dark:text-zinc-300">
              {{ t(nodeKindMeta[row.node.kind].labelKey) }}
            </div>

            <!-- 目标 / 参数 -->
            <div class="overflow-hidden px-1">
              <div
                v-if="!row.isGroup"
                class="truncate font-mono text-[11px] text-zinc-500 dark:text-zinc-400"
                :title="targetText(row.node)"
              >
                {{ targetText(row.node) }}
              </div>
              <span v-else class="text-[11px] text-zinc-400 dark:text-zinc-600">—</span>
            </div>

            <!-- 上游依赖 -->
            <div class="overflow-hidden px-1">
              <template v-if="row.isGroup">
                <div
                  v-if="externalUpstreamOf(row.node.id).length > 0"
                  class="truncate text-[11px] text-zinc-500 dark:text-zinc-400"
                  :title="
                    externalUpstreamOf(row.node.id)
                      .map((item) => `${item.name} · ${item.condition}`)
                      .join('，')
                  "
                >
                  <template
                    v-for="(item, index) in externalUpstreamOf(row.node.id)"
                    :key="item.key"
                  >
                    <span v-if="index > 0" class="text-zinc-300 dark:text-zinc-600">
                      ，
                    </span>
                    <span class="text-zinc-600 dark:text-zinc-300">{{ item.name }}</span>
                    <span class="text-zinc-400 dark:text-zinc-500">·{{ item.condition }}</span>
                  </template>
                </div>
                <span v-else class="text-[11px] text-zinc-400 dark:text-zinc-600">
                  {{ t("workflow.text.noDependencies") }}
                </span>
              </template>
              <template v-else>
                <div
                  v-if="upstreamOf(row.node.id).length > 0"
                  class="truncate text-[11px] text-zinc-500 dark:text-zinc-400"
                  :title="
                    upstreamOf(row.node.id)
                      .map((item) => `${item.name} · ${item.condition}`)
                      .join('，')
                  "
                >
                  <template
                    v-for="(item, index) in upstreamOf(row.node.id)"
                    :key="item.id"
                  >
                    <span v-if="index > 0" class="text-zinc-300 dark:text-zinc-600">
                      ，
                    </span>
                    <span class="text-zinc-600 dark:text-zinc-300">{{ item.name }}</span>
                    <span class="text-zinc-400 dark:text-zinc-500">·{{ item.condition }}</span>
                  </template>
                </div>
                <span v-else class="text-[11px] text-zinc-400 dark:text-zinc-600">
                  {{ t("workflow.text.noDependencies") }}
                </span>
              </template>
            </div>

            <!-- 下游数 -->
            <div class="px-1 text-center tabular text-zinc-500 dark:text-zinc-400">
              <template v-if="!row.isGroup">{{ downstreamCount(row.node.id) }}</template>
              <template v-else>—</template>
            </div>

            <!-- 状态 -->
            <div class="overflow-hidden px-1">
              <WorkflowStatusBadge :status="rowStatus(row)" />
            </div>
          </div>
        </div>
      </div>
    </div>

    <!-- 执行分层 -->
    <div class="shrink-0 border-t border-zinc-200 dark:border-zinc-800">
      <div class="flex items-center justify-between px-3 pb-1 pt-2">
        <p
          class="inline-flex items-center gap-1.5 text-[10px] font-semibold uppercase tracking-wider text-zinc-400 dark:text-zinc-500"
        >
          <Layers class="h-3 w-3" />
          {{ t("workflow.text.layers") }}
        </p>
        <span class="tabular text-[10px] text-zinc-400 dark:text-zinc-500">
          {{ layerInfo.count }}
        </span>
      </div>
      <div class="max-h-40 overflow-y-auto px-3 pb-2">
        <div
          v-for="(ids, layerIndex) in layerInfo.layers"
          :key="layerIndex"
          class="flex items-start gap-2 py-0.5"
        >
          <span
            class="w-14 shrink-0 font-mono text-[11px] leading-5 text-zinc-400 dark:text-zinc-500"
          >
            {{ t("workflow.text.layer") }} {{ layerIndex + 1 }}
          </span>
          <div class="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-0.5">
            <button
              v-for="nodeId in ids"
              :key="nodeId"
              type="button"
              class="max-w-[180px] truncate rounded px-1 font-mono text-[11px] leading-5 transition hover:bg-zinc-100 dark:hover:bg-zinc-800"
              :class="[
                nodeStatusText[statusOf(nodeId)],
                nodeId === props.selectedNodeId
                  ? 'bg-brand-50 dark:bg-brand-500/15'
                  : '',
              ]"
              :title="nameOf(nodeId)"
              @click="emit('select', nodeId)"
            >
              {{ nameOf(nodeId) }}
            </button>
          </div>
        </div>
        <p v-if="layerInfo.count === 0" class="py-1 text-[11px] text-zinc-400 dark:text-zinc-600">
          —
        </p>
      </div>
    </div>
  </div>
</template>
