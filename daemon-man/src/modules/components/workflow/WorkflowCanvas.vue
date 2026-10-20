<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from "vue";
import {
  Panel,
  useVueFlow,
  VueFlow,
  type Connection,
  type Edge,
  type EdgeChange,
  type EdgeRemoveChange,
  type GraphEdge,
  type GraphNode,
  type Node,
  type NodeChange,
  type NodeDragEvent,
  type NodeRemoveChange,
} from "@vue-flow/core";
import { Background } from "@vue-flow/additional-components";
import { GitBranch, LayoutGrid, Maximize2, Minus, Plus } from "lucide-vue-next";

import type { NodeKind, NodePosition, Workflow, WorkflowEdge } from "@/api";
import BaseButton from "@/modules/components/ui/BaseButton.vue";
import EmptyState from "@/modules/components/ui/EmptyState.vue";
import { useI18n } from "@/modules/locales";
import { useThemeStore } from "@/modules/stores/theme";
import { useUiStore } from "@/modules/stores/ui";
import { useWorkflowStore } from "@/modules/stores/workflows";

import WorkflowEdgeLine from "./edges/WorkflowEdgeLine.vue";
import { buildFlowEdge, buildFlowNode, createWorkflowNode, edgeIdFor } from "./factory";
import { nodeKindMeta, nodeKindOrder } from "./meta";
import WorkflowNodeCard from "./nodes/WorkflowNodeCard.vue";
import type { WorkflowSelection } from "./types";

const emit = defineEmits<{ "selection-change": [selection: WorkflowSelection] }>();

/** 外部（比如从文本视图切回来）希望保持的选中节点。 */
const props = withDefaults(
  defineProps<{ selectedNodeId?: string | null }>(),
  { selectedNodeId: null },
);

const store = useWorkflowStore();
const ui = useUiStore();
const theme = useThemeStore();
const { t } = useI18n();

const {
  addEdges,
  addNodes,
  addSelectedEdges,
  addSelectedNodes,
  dimensions,
  findEdge,
  findNode,
  fitView,
  getSelectedEdges,
  getSelectedNodes,
  removeEdges,
  removeNodes,
  removeSelectedElements,
  updateNode,
  viewport,
  zoomIn,
  zoomOut,
} = useVueFlow();

/** 画布本地状态，用 v-model 与 Vue Flow 内部 store 双向同步。 */
const flowNodes = ref<Node[]>([]);
const flowEdges = ref<Edge[]>([]);

const draft = computed(() => store.draft);
const isEmpty = computed(
  () => !!draft.value && draft.value.nodes.length === 0,
);

const gridColor = computed(() => (theme.isDark ? "#27272a" : "#e4e4e7"));

/** 固定引用，避免每次渲染都生成新对象 / 数组导致 Vue Flow 重复应用 props。 */
const DEFAULT_VIEWPORT = { zoom: 1 };
const DELETE_KEYS = ["Delete", "Backspace"];

/** 上一次重建时的 draft id，用于判断是否需要 fitView。 */
let lastDraftId: string | null = null;
let unmounted = false;

onBeforeUnmount(() => {
  unmounted = true;
});

/**
 * 节点刚写入时 Vue Flow 还没量出尺寸，fitView 会直接返回 false；
 * 这里渲染后重试几次，保证切换工作流后视图一定会贴合内容。
 */
function fitWhenReady(attempts = 4) {
  if (unmounted) return;
  void nextTick(() => {
    if (unmounted) return;
    void fitView({ padding: 0.22, duration: 200 }).then((done) => {
      if (done || unmounted || attempts <= 1) return;
      window.setTimeout(() => fitWhenReady(attempts - 1), 120);
    });
  });
}

function emitSelection() {
  const nodes = getSelectedNodes.value;
  const edges = getSelectedEdges.value;
  emit("selection-change", {
    nodeId: nodes.length === 1 ? nodes[0].id : null,
    edgeId:
      nodes.length === 0 && edges.length === 1 ? edges[0].id : null,
  });
}

/** 从文本视图切回节点视图时画布会重新挂载，这里按外部选中恢复画布选中态。 */
function applySelectedNode(id: string | null | undefined) {
  if (!id) return;
  const current = getSelectedNodes.value;
  if (current.length === 1 && current[0].id === id) return;
  const node = findNode(id);
  if (!node) return;
  removeSelectedElements();
  addSelectedNodes([node]);
}

watch(
  () => props.selectedNodeId,
  (id) => {
    // 只在明确选中某个节点时同步；null 不主动清空，避免把「选中边」也一起取消。
    if (id) applySelectedNode(id);
  },
  { immediate: true, flush: "post" },
);

/** 从 draft 全量重建画布（仅在 draft 对象整体替换时调用：载入 / 保存 / 切换）。 */
function rebuildFromDraft(next: Workflow | null) {
  const idChanged = (next?.id ?? null) !== lastDraftId;
  lastDraftId = next?.id ?? null;

  if (!next) {
    flowNodes.value = [];
    flowEdges.value = [];
    emitSelection();
    return;
  }

  const selectedNodeIds = getSelectedNodes.value.map((node) => node.id);
  const selectedEdgeIds = getSelectedEdges.value.map((edge) => edge.id);

  flowNodes.value = next.nodes.map((node) => buildFlowNode(node));
  flowEdges.value = next.edges.map((edge) => buildFlowEdge(edge, theme.isDark));

  void nextTick(() => {
    // 重建会丢失内部选中态，这里恢复一下，避免保存后属性面板突然清空。
    const nodes = selectedNodeIds
      .map((id) => findNode(id))
      .filter((node): node is GraphNode => Boolean(node));
    if (nodes.length > 0) {
      addSelectedNodes(nodes);
    }
    const edges = selectedEdgeIds
      .map((id) => findEdge(id))
      .filter((edge): edge is GraphEdge => Boolean(edge));
    if (edges.length > 0) {
      addSelectedEdges(edges);
    }

    if (idChanged && next.nodes.length > 0) {
      fitWhenReady();
    }
    emitSelection();
  });
}

// draft 只在 loadDraft / saveDraft / createDraft / discardDraft 时整体替换，
// 字段级编辑通过 data.node 引用直接反映，不需要重建画布（也就不会打断拖拽）。
watch(() => store.draft, (next) => rebuildFromDraft(next), { immediate: true });

// 节点类型切换：kind 就是画布节点 type，直接就地更新 store 中的节点。
watch(
  () => (store.draft?.nodes ?? []).map((node) => `${node.id}:${node.kind}`).join("|"),
  () => {
    if (!store.draft) return;
    for (const node of store.draft.nodes) {
      const current = findNode(node.id);
      if (current && current.type !== node.kind) {
        updateNode(node.id, { type: node.kind });
      }
    }
  },
);

// 主题切换时重算连线箭头颜色。
watch(
  () => theme.isDark,
  () => {
    if (!store.draft) return;
    flowEdges.value = store.draft.edges.map((edge) =>
      buildFlowEdge(edge, theme.isDark),
    );
  },
);

watch([getSelectedNodes, getSelectedEdges], () => emitSelection(), {
  flush: "post",
});

function onInit() {
  applySelectedNode(props.selectedNodeId);
  if ((store.draft?.nodes.length ?? 0) > 0) {
    fitWhenReady();
  }
}

function onConnect(connection: Connection) {
  const current = store.draft;
  if (!current) return;
  const { source, target } = connection;
  if (!source || !target || source === target) return;
  if (current.edges.some((edge) => edge.source === source && edge.target === target)) {
    ui.notify("info", t("workflow.edgeDuplicate"));
    return;
  }
  const edge: WorkflowEdge = {
    id: edgeIdFor(source, target),
    source,
    target,
    condition: "success",
  };
  current.edges.push(edge);
  addEdges(buildFlowEdge(edge, theme.isDark));
}

/** Vue Flow 内置删除（Delete / Backspace）只会改内部 state，这里同步回 draft。 */
function onNodesChange(changes: NodeChange[]) {
  const current = store.draft;
  if (!current) return;
  const removedIds = changes
    .filter((change): change is NodeRemoveChange => change.type === "remove")
    .map((change) => change.id);
  if (removedIds.length === 0) return;
  current.nodes = current.nodes.filter((node) => !removedIds.includes(node.id));
  current.edges = current.edges.filter(
    (edge) => !removedIds.includes(edge.source) && !removedIds.includes(edge.target),
  );
}

function onEdgesChange(changes: EdgeChange[]) {
  const current = store.draft;
  if (!current) return;
  const removedIds = changes
    .filter((change): change is EdgeRemoveChange => change.type === "remove")
    .map((change) => change.id);
  if (removedIds.length === 0) return;
  current.edges = current.edges.filter((edge) => !removedIds.includes(edge.id));
}

function onNodeDragStop(event: NodeDragEvent) {
  const current = store.draft;
  if (!current) return;
  for (const dragged of event.nodes) {
    const node = current.nodes.find((item) => item.id === dragged.id);
    if (!node) continue;
    node.position = {
      x: Math.round(dragged.position.x),
      y: Math.round(dragged.position.y),
    };
  }
}

/** 新节点落在视口中心附近，并按已有节点数量做一点错位。 */
function nextNodePosition(): NodePosition {
  const zoom = viewport.value.zoom || 1;
  const width = dimensions.value.width || 800;
  const height = dimensions.value.height || 480;
  const centerX = (width / 2 - viewport.value.x) / zoom;
  const centerY = (height / 2 - viewport.value.y) / zoom;
  const offset = (store.draft?.nodes.length ?? 0) % 4;
  return {
    x: Math.round(centerX - 112 + offset * 18),
    y: Math.round(centerY - 34 + offset * 18),
  };
}

function addNode(kind: NodeKind) {
  const current = store.draft;
  if (!current) return;
  const meta = nodeKindMeta[kind];
  const node = createWorkflowNode(kind, nextNodePosition());
  const index = current.nodes.filter((item) => item.kind === kind).length + 1;
  node.name = `${t(meta.labelKey)} ${index}`;
  current.nodes.push(node);
  addNodes(buildFlowNode(node));
  void nextTick(() => {
    const added = findNode(node.id);
    if (added) addSelectedNodes([added]);
  });
}

function removeNodeById(id: string) {
  const current = store.draft;
  if (current) {
    current.nodes = current.nodes.filter((node) => node.id !== id);
    current.edges = current.edges.filter(
      (edge) => edge.source !== id && edge.target !== id,
    );
  }
  removeNodes([id]);
}

function removeEdgeById(id: string) {
  const current = store.draft;
  if (current) {
    current.edges = current.edges.filter((edge) => edge.id !== id);
  }
  removeEdges([id]);
}

/** 简单分层布局：按入边拓扑层级排布，列间距 280、行间距 120。 */
function autoLayout() {
  const current = store.draft;
  if (!current || current.nodes.length === 0) return;

  const nodeIds = new Set(current.nodes.map((node) => node.id));
  const indegree = new Map<string, number>();
  current.nodes.forEach((node) => indegree.set(node.id, 0));
  current.edges.forEach((edge) => {
    if (!nodeIds.has(edge.source) || !nodeIds.has(edge.target)) return;
    indegree.set(edge.target, (indegree.get(edge.target) ?? 0) + 1);
  });

  const levels = new Map<string, number>();
  const remaining = new Map(indegree);
  const queue = current.nodes
    .filter((node) => (indegree.get(node.id) ?? 0) === 0)
    .map((node) => node.id);
  queue.forEach((id) => levels.set(id, 0));

  while (queue.length > 0) {
    const id = queue.shift() as string;
    const level = levels.get(id) ?? 0;
    for (const edge of current.edges) {
      if (edge.source !== id || !nodeIds.has(edge.target)) continue;
      levels.set(edge.target, Math.max(levels.get(edge.target) ?? 0, level + 1));
      const left = (remaining.get(edge.target) ?? 1) - 1;
      remaining.set(edge.target, left);
      if (left <= 0) queue.push(edge.target);
    }
  }

  const maxLevel = Math.max(0, ...Array.from(levels.values()));
  current.nodes.forEach((node) => {
    if (!levels.has(node.id)) levels.set(node.id, maxLevel + 1);
  });

  const columnCounts = new Map<number, number>();
  for (const node of current.nodes) {
    const level = levels.get(node.id) ?? 0;
    const indexInColumn = columnCounts.get(level) ?? 0;
    columnCounts.set(level, indexInColumn + 1);
    const position: NodePosition = {
      x: 80 + level * 280,
      y: 60 + indexInColumn * 120,
    };
    node.position = position;
    updateNode(node.id, { position });
  }

  void nextTick(() => {
    fitWhenReady(2);
  });
}

function fitViewToContent() {
  fitWhenReady(2);
}

defineExpose({ addNode, removeNodeById, removeEdgeById, autoLayout, fitViewToContent });
</script>

<template>
  <div class="relative h-full w-full">
    <VueFlow
      v-model:nodes="flowNodes"
      v-model:edges="flowEdges"
      class="h-full w-full"
      :default-viewport="DEFAULT_VIEWPORT"
      :min-zoom="0.25"
      :max-zoom="2"
      :delete-key-code="DELETE_KEYS"
      :zoom-on-double-click="false"
      :elevate-edges-on-select="true"
      :edges-updatable="false"
      :default-marker-color="theme.isDark ? '#818cf8' : '#6366f1'"
      @init="onInit"
      @connect="onConnect"
      @nodes-change="onNodesChange"
      @edges-change="onEdgesChange"
      @node-drag-stop="onNodeDragStop"
    >
      <template #node-application="nodeProps">
        <WorkflowNodeCard v-bind="nodeProps" />
      </template>
      <template #node-delay="nodeProps">
        <WorkflowNodeCard v-bind="nodeProps" />
      </template>
      <template #node-wait="nodeProps">
        <WorkflowNodeCard v-bind="nodeProps" />
      </template>
      <template #node-script="nodeProps">
        <WorkflowNodeCard v-bind="nodeProps" />
      </template>
      <template #edge-workflow="edgeProps">
        <WorkflowEdgeLine v-bind="edgeProps" />
      </template>

      <Background :gap="18" :size="1" :pattern-color="gridColor" />

      <!-- 节点面板 -->
      <Panel
        position="top-left"
        class="w-44 rounded-xl border border-zinc-200 bg-white/95 p-2 shadow-sm backdrop-blur dark:border-zinc-800 dark:bg-zinc-900/95"
      >
        <p
          class="px-1 pb-1.5 text-[10px] font-semibold uppercase tracking-wider text-zinc-400 dark:text-zinc-500"
        >
          {{ t("workflow.nodePalette") }}
        </p>
        <div class="grid grid-cols-2 gap-1">
          <button
            v-for="kind in nodeKindOrder"
            :key="kind"
            type="button"
            class="flex flex-col items-start gap-1 rounded-lg border border-transparent px-1.5 py-1.5 text-left transition hover:border-zinc-200 hover:bg-zinc-50 dark:hover:border-zinc-700 dark:hover:bg-zinc-800/60"
            :title="t(nodeKindMeta[kind].hintKey)"
            @click="addNode(kind)"
          >
            <span
              class="flex h-6 w-6 items-center justify-center rounded-md"
              :class="nodeKindMeta[kind].iconClass"
            >
              <component :is="nodeKindMeta[kind].icon" class="h-3.5 w-3.5" />
            </span>
            <span
              class="text-[11px] font-medium leading-tight text-zinc-600 dark:text-zinc-300"
            >
              {{ t(nodeKindMeta[kind].addLabelKey) }}
            </span>
          </button>
        </div>
      </Panel>

      <!-- 缩放 / 布局控件 -->
      <Panel
        position="bottom-left"
        class="flex items-center gap-0.5 rounded-lg border border-zinc-200 bg-white/95 p-1 shadow-sm backdrop-blur dark:border-zinc-800 dark:bg-zinc-900/95"
      >
        <BaseButton
          variant="ghost"
          size="sm"
          icon-only
          :title="t('workflow.zoomOut')"
          @click="zoomOut({ duration: 120 })"
        >
          <Minus class="h-3.5 w-3.5" />
        </BaseButton>
        <BaseButton
          variant="ghost"
          size="sm"
          icon-only
          :title="t('workflow.zoomIn')"
          @click="zoomIn({ duration: 120 })"
        >
          <Plus class="h-3.5 w-3.5" />
        </BaseButton>
        <BaseButton
          variant="ghost"
          size="sm"
          icon-only
          :title="t('workflow.fitView')"
          @click="fitViewToContent"
        >
          <Maximize2 class="h-3.5 w-3.5" />
        </BaseButton>
        <span class="mx-0.5 h-4 w-px bg-zinc-200 dark:bg-zinc-700" />
        <BaseButton
          variant="ghost"
          size="sm"
          :title="t('workflow.autoLayout')"
          @click="autoLayout"
        >
          <LayoutGrid class="h-3.5 w-3.5" />
          <span class="text-[11px]">{{ t("workflow.autoLayout") }}</span>
        </BaseButton>
      </Panel>
    </VueFlow>

    <!-- 画布为空时的引导 -->
    <div
      v-if="isEmpty"
      class="pointer-events-none absolute inset-0 z-10 flex items-center justify-center p-6"
    >
      <div
        class="pointer-events-none rounded-xl bg-white/70 px-6 py-2 backdrop-blur-sm dark:bg-zinc-950/60"
      >
        <EmptyState
          :title="t('workflow.noNodes')"
          :description="t('workflow.noNodesHint')"
        >
          <template #icon>
            <GitBranch class="h-7 w-7" />
          </template>
        </EmptyState>
      </div>
    </div>
  </div>
</template>
