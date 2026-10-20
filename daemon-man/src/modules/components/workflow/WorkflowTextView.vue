<script setup lang="ts">
import { computed } from "vue";
import { Layers, ListTree, Workflow } from "lucide-vue-next";

import type { Workflow as WorkflowModel, WorkflowNode } from "@/api";
import BaseBadge from "@/modules/components/ui/BaseBadge.vue";
import EmptyState from "@/modules/components/ui/EmptyState.vue";
import { useI18n } from "@/modules/locales";
import { useWorkflowStore } from "@/modules/stores/workflows";
import { nodeStatusVariant } from "@/modules/utils/status";

import { buildAdjacency, computeExecutionLayers } from "./graph";
import { edgeConditionLabel, nodeKindMeta, nodeStatusDot, nodeStatusText } from "./meta";

const props = defineProps<{
  draft: WorkflowModel | null;
  selectedNodeId: string | null;
}>();

const emit = defineEmits<{ select: [nodeId: string] }>();

const store = useWorkflowStore();
const { t } = useI18n();

const nodes = computed<WorkflowNode[]>(() => props.draft?.nodes ?? []);
const edges = computed(() => props.draft?.edges ?? []);

const adjacency = computed(() => buildAdjacency(nodes.value, edges.value));
const layerInfo = computed(() => computeExecutionLayers(nodes.value, edges.value));

const isEmpty = computed(() => nodes.value.length === 0);

/** 按执行层排序后的行（层内保持 draft 中的顺序），`position` 为展示序号。 */
const rows = computed(() => {
  const indexed = nodes.value.map((node, index) => ({
    node,
    index,
    layer: layerInfo.value.layerOf.get(node.id) ?? 1,
  }));
  indexed.sort((left, right) => left.layer - right.layer || left.index - right.index);
  return indexed.map((item, position) => ({ ...item, position: position + 1 }));
});

const summaryText = computed(() =>
  t("workflow.text.summary", {
    nodes: nodes.value.length,
    edges: edges.value.length,
    layers: layerInfo.value.count,
  }),
);

function nameOf(nodeId: string): string {
  const node = nodes.value.find((item) => item.id === nodeId);
  if (!node) return "—";
  return node.name || t(nodeKindMeta[node.kind].labelKey);
}

function statusOf(nodeId: string) {
  return store.nodeRun(nodeId)?.status ?? "pending";
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

function layerNodes(layerIndex: number): WorkflowNode[] {
  const ids = layerInfo.value.layers[layerIndex] ?? [];
  return ids
    .map((id) => nodes.value.find((node) => node.id === id))
    .filter((node): node is WorkflowNode => Boolean(node));
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
    </div>

    <!-- 节点清单 -->
    <div class="flex min-h-0 flex-1 flex-col">
      <div class="flex shrink-0 items-center justify-between px-3 pb-1 pt-2">
        <p
          class="text-[10px] font-semibold uppercase tracking-wider text-zinc-400 dark:text-zinc-500"
        >
          {{ t("workflow.text.nodes") }}
        </p>
        <span class="tabular text-[10px] text-zinc-400 dark:text-zinc-500">
          {{ rows.length }}
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
        <table class="w-full min-w-[720px] table-fixed border-collapse text-xs">
          <thead>
            <tr
              class="h-7 text-left text-[10px] font-semibold uppercase tracking-wider text-zinc-400 dark:text-zinc-500"
            >
              <th class="sticky top-0 z-10 w-8 bg-zinc-50 px-2 dark:bg-zinc-900">
                {{ t("workflow.text.index") }}
              </th>
              <th class="sticky top-0 z-10 w-10 bg-zinc-50 px-2 dark:bg-zinc-900">
                {{ t("workflow.text.layer") }}
              </th>
              <th class="sticky top-0 z-10 w-20 bg-zinc-50 px-2 dark:bg-zinc-900">
                {{ t("workflow.text.type") }}
              </th>
              <th class="sticky top-0 z-10 w-28 bg-zinc-50 px-2 dark:bg-zinc-900">
                {{ t("workflow.text.name") }}
              </th>
              <th class="sticky top-0 z-10 bg-zinc-50 px-2 dark:bg-zinc-900">
                {{ t("workflow.text.target") }}
              </th>
              <th class="sticky top-0 z-10 w-40 bg-zinc-50 px-2 dark:bg-zinc-900">
                {{ t("workflow.text.dependencies") }}
              </th>
              <th class="sticky top-0 z-10 w-16 bg-zinc-50 px-2 dark:bg-zinc-900">
                {{ t("workflow.text.downstream") }}
              </th>
              <th class="sticky top-0 z-10 w-20 bg-zinc-50 px-2 dark:bg-zinc-900">
                {{ t("workflow.text.status") }}
              </th>
            </tr>
          </thead>
          <tbody>
            <tr
              v-for="row in rows"
              :key="row.node.id"
              tabindex="0"
              class="h-7 cursor-pointer border-b border-zinc-100 transition-colors dark:border-zinc-800/60"
              :class="[
                row.node.id === props.selectedNodeId
                  ? 'bg-brand-50 dark:bg-brand-500/10'
                  : 'hover:bg-zinc-50 dark:hover:bg-zinc-800/50',
                row.node.enabled ? '' : 'opacity-50',
              ]"
              @click="emit('select', row.node.id)"
              @keydown.enter.prevent="emit('select', row.node.id)"
            >
              <td class="overflow-hidden px-2 text-zinc-400 tabular dark:text-zinc-500">
                {{ row.position }}
              </td>
              <td class="overflow-hidden px-2">
                <span
                  class="inline-flex h-4 min-w-4 items-center justify-center rounded bg-zinc-100 px-1 text-[10px] font-medium text-zinc-500 dark:bg-zinc-800 dark:text-zinc-400"
                >
                  {{ row.layer }}
                </span>
              </td>
              <td class="overflow-hidden px-2">
                <div class="flex min-w-0 items-center gap-1">
                  <span
                    class="flex h-4 w-4 shrink-0 items-center justify-center rounded"
                    :class="nodeKindMeta[row.node.kind].iconClass"
                  >
                    <component
                      :is="nodeKindMeta[row.node.kind].icon"
                      class="h-2.5 w-2.5"
                    />
                  </span>
                  <span class="truncate text-zinc-600 dark:text-zinc-300">
                    {{ t(nodeKindMeta[row.node.kind].labelKey) }}
                  </span>
                </div>
              </td>
              <td class="overflow-hidden px-2">
                <div class="flex min-w-0 items-center gap-1">
                  <span
                    class="h-1.5 w-1.5 shrink-0 rounded-full"
                    :class="nodeStatusDot[statusOf(row.node.id)]"
                  />
                  <span
                    class="truncate font-medium"
                    :class="nodeStatusText[statusOf(row.node.id)]"
                    :title="nameOf(row.node.id)"
                  >
                    {{ nameOf(row.node.id) }}
                  </span>
                </div>
              </td>
              <td class="overflow-hidden px-2">
                <div
                  class="truncate font-mono text-[11px] text-zinc-500 dark:text-zinc-400"
                  :title="targetText(row.node)"
                >
                  {{ targetText(row.node) }}
                </div>
              </td>
              <td class="overflow-hidden px-2">
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
              </td>
              <td class="overflow-hidden px-2 tabular text-zinc-500 dark:text-zinc-400">
                {{ downstreamCount(row.node.id) }}
              </td>
              <td class="overflow-hidden px-2">
                <BaseBadge
                  :variant="nodeStatusVariant(statusOf(row.node.id))"
                  dot
                  :pulse="statusOf(row.node.id) === 'running'"
                >
                  {{ t(`status.${statusOf(row.node.id)}`) }}
                </BaseBadge>
              </td>
            </tr>
          </tbody>
        </table>
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
              v-for="node in layerNodes(layerIndex)"
              :key="node.id"
              type="button"
              class="max-w-[180px] truncate rounded px-1 font-mono text-[11px] leading-5 transition hover:bg-zinc-100 dark:hover:bg-zinc-800"
              :class="[
                nodeStatusText[statusOf(node.id)],
                node.enabled ? '' : 'opacity-50 line-through',
                node.id === props.selectedNodeId
                  ? 'bg-brand-50 dark:bg-brand-500/15'
                  : '',
              ]"
              :title="nameOf(node.id)"
              @click="emit('select', node.id)"
            >
              {{ nameOf(node.id) }}
            </button>
            <span
              v-if="ids.length === 0"
              class="text-[11px] text-zinc-400 dark:text-zinc-600"
            >
              —
            </span>
          </div>
        </div>
        <p
          v-if="layerInfo.count === 0"
          class="py-1 text-[11px] text-zinc-400 dark:text-zinc-600"
        >
          —
        </p>
      </div>
    </div>
  </div>
</template>
