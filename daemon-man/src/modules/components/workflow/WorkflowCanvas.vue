<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, provide, ref, watch } from "vue";
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
  type EdgeMouseEvent,
  type NodeMouseEvent,
  type NodeRemoveChange,
} from "@vue-flow/core";
import { Background } from "@vue-flow/additional-components";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import type { UnlistenFn } from "@tauri-apps/api/event";
import {
  GitBranch,
  LayoutGrid,
  Magnet,
  Maximize2,
  Minus,
  Play,
  Plus,
} from "lucide-vue-next";

import type {
  NodeKind,
  NodePosition,
  StartMenuApp,
  Workflow,
  WorkflowEdge,
  WorkflowNode,
} from "@/api";
import BaseButton from "@/modules/components/ui/BaseButton.vue";
import EmptyState from "@/modules/components/ui/EmptyState.vue";
import { useI18n } from "@/modules/locales";
import { useThemeStore } from "@/modules/stores/theme";
import { useUiStore } from "@/modules/stores/ui";
import { useWorkflowStore } from "@/modules/stores/workflows";

import WorkflowEdgeLine from "./edges/WorkflowEdgeLine.vue";
import { runSingleNode } from "./commands";
import {
  DELAY_PILL_HEIGHT,
  DELAY_PILL_WIDTH,
  buildDelayLayoutGraph,
  computeDelayPills,
  pillByEdge,
  placeDelayPills,
} from "./delayPill";
import {
  buildFlowEdge,
  buildFlowNode,
  createWorkflowNode,
  edgeIdFor,
  fileBaseName,
  fileExtension,
} from "./factory";
import { computeTopology } from "./graph";
import {
  DEFAULT_GROUP_HEIGHT,
  DEFAULT_GROUP_WIDTH,
  GROUP_HEADER,
  GROUP_PADDING,
  NODE_HEIGHT,
  NODE_WIDTH,
  computeAutoLayout,
  computeGroupPlans,
} from "./layout";
import { nodeKindMeta, nodeKindOrder } from "./meta";
import WorkflowDelayPill from "./nodes/WorkflowDelayPill.vue";
import WorkflowGroupNode from "./nodes/WorkflowGroupNode.vue";
import WorkflowNodeCard from "./nodes/WorkflowNodeCard.vue";
import { errorText } from "./shared";
import { WORKFLOW_STEPS, type WorkflowSelection } from "./types";

const emit = defineEmits<{
  "selection-change": [selection: WorkflowSelection];
  /** 节点面板点了「启动应用」：请外层打开开始菜单选择器。 */
  "pick-app": [];
}>();

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
  screenToFlowCoordinate,
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

// ---------------------------------------------------------------------------
// 吸附（snap 16px，默认开启，localStorage 记忆）
// ---------------------------------------------------------------------------
const SNAP_STORAGE_KEY = "daemon-man-workflow-snap";
const SNAP_GRID: [number, number] = [16, 16];

function readSnapPreference(): boolean {
  try {
    return localStorage.getItem(SNAP_STORAGE_KEY) !== "0";
  } catch {
    return true;
  }
}

const snapEnabled = ref(readSnapPreference());

function toggleSnap() {
  snapEnabled.value = !snapEnabled.value;
  try {
    localStorage.setItem(SNAP_STORAGE_KEY, snapEnabled.value ? "1" : "0");
  } catch {
    // 忽略隐私模式等写入失败。
  }
}

const snapLabel = computed(() => {
  const value = t("workflow.snap");
  return value === "workflow.snap" ? "吸附 16px" : value;
});

const snapHint = computed(() => {
  const value = t("workflow.snapHint");
  return value === "workflow.snapHint" ? "拖动节点时按 16px 网格对齐" : value;
});

/** 执行步骤号（拓扑序），通过 provide 给节点卡片显示 `#N` 徽标。 */
const stepOf = ref<Map<string, number>>(new Map());
/** 执行层，用于区分「执行流」与「依赖跳线」。 */
const layerMap = ref<Map<string, number>>(new Map());
provide(WORKFLOW_STEPS, stepOf);

function applyTopology(draft: Workflow) {
  const pills = computeDelayPills(draft.nodes, draft.edges);
  if (pills.size === 0) {
    const topology = computeTopology(draft.nodes, draft.edges);
    stepOf.value = topology.stepOf;
    layerMap.value = topology.layerOf;
    return;
  }
  // 步骤号：把折叠成药丸的延时排除在外，可见节点保持连续编号（#1 → #2，不再断档）。
  const graph = buildDelayLayoutGraph(draft.nodes, draft.edges, pills);
  stepOf.value = computeTopology(graph.nodes, graph.edges).stepOf;
  // 连线分类仍按完整图算：药丸两侧的边要按真实层级判定为执行流（实线）。
  layerMap.value = computeTopology(draft.nodes, draft.edges).layerOf;
}

/** 相邻层 + success 才算执行流；跨层、同层、失败/总是都算依赖跳线。 */
function edgeIsFlow(edge: WorkflowEdge): boolean {
  if (edge.condition !== "success") return false;
  const source = layerMap.value.get(edge.source);
  const target = layerMap.value.get(edge.target);
  if (source === undefined || target === undefined) return false;
  return target === source + 1;
}

function buildEdgesFor(draft: Workflow): Edge[] {
  return draft.edges.map((edge) =>
    buildFlowEdge(edge, theme.isDark, edgeIsFlow(edge)),
  );
}

/** 拓扑变化后重算步骤号与连线分类（保持边的选中态）。 */
let refreshToken = 0;
let suppressSelectionEmit = false;

/**
 * 依据当前图结构同步节点类型：
 * - 候选「连线延时」→ delay-pill（药丸，尺寸 88×28）；
 * - 不再候选 → 恢复普通 delay 卡片；
 * - 其它 kind 切换照旧。
 */
function syncFlowNodeTypes() {
  const draft = store.draft;
  if (!draft) return;
  const pills = computeDelayPills(draft.nodes, draft.edges);
  for (const node of draft.nodes) {
    const current = findNode(node.id);
    if (!current) continue;
    const isPill = node.kind === "delay" && pills.has(node.id);
    const expectedType = isPill ? "delay-pill" : node.kind;
    if (current.type === expectedType) continue;

    const update: Partial<Node> = { type: expectedType };
    if (node.kind === "group") {
      update.connectable = false;
    } else {
      // 药丸锁定在连线上：不可拖拽 / 不可连线；恢复卡片时再打开。
      update.draggable = !isPill;
      update.connectable = !isPill;
    }
    if (isPill) {
      update.style = {
        width: `${DELAY_PILL_WIDTH}px`,
        height: `${DELAY_PILL_HEIGHT}px`,
      };
    } else if (current.type === "delay-pill") {
      update.style = {};
    }
    updateNode(node.id, update);
  }
}

function refreshFlowEdges() {
  const current = store.draft;
  if (!current) return;
  applyTopology(current);
  syncFlowNodeTypes();
  const selectedIds = getSelectedEdges.value.map((edge) => edge.id);
  const token = ++refreshToken;
  // 重建期间先屏蔽选中回传，避免右栏属性面板被瞬间清空。
  suppressSelectionEmit = true;
  flowEdges.value = buildEdgesFor(current);
  void nextTick(() => {
    if (token !== refreshToken) return;
    const edges = selectedIds
      .map((id) => findEdge(id))
      .filter((edge): edge is GraphEdge => Boolean(edge));
    if (edges.length > 0) addSelectedEdges(edges);
    suppressSelectionEmit = false;
    emitSelection();
  });
}

interface GroupBox {
  x: number;
  y: number;
  width: number;
  height: number;
}

/** 分组容器的最新边界（由子节点位置/尺寸推导）。 */
const groupBoxes = ref<Map<string, GroupBox>>(new Map());

/** 正在被拖动的分组 id：拖动期间该组的几何位置交给拖拽逻辑，避免互相抢。 */
const draggingGroupId = ref<string | null>(null);

interface GroupDragState {
  groupId: string;
  last: NodePosition;
  /** 分组的所有后代节点（含嵌套分组及其子项） */
  subtree: string[];
}

let groupDragState: GroupDragState | null = null;

/** 收集分组的所有后代节点 id（用于整组平移）。 */
function collectSubtreeIds(groupId: string): string[] {
  const draft = store.draft;
  if (!draft) return [];
  const childrenByParent = new Map<string, string[]>();
  for (const node of draft.nodes) {
    if (!node.parentId) continue;
    const list = childrenByParent.get(node.parentId);
    if (list) list.push(node.id);
    else childrenByParent.set(node.parentId, [node.id]);
  }
  const result: string[] = [];
  const seen = new Set<string>();
  const stack = [...(childrenByParent.get(groupId) ?? [])];
  while (stack.length > 0) {
    const id = stack.pop() as string;
    if (seen.has(id)) continue;
    seen.add(id);
    result.push(id);
    stack.push(...(childrenByParent.get(id) ?? []));
  }
  return result;
}

/** 上一次的结构快照，用于识别「子项被加入/移出分组」「并行开关变化」。 */
const lastGroupSignature = new Map<string, string>();
const lastParentOf = new Map<string, string | null>();

let geometryFrame = 0;

function flowPositionOf(node: WorkflowNode): NodePosition {
  const flow = findNode(node.id);
  return flow
    ? { x: flow.position.x, y: flow.position.y }
    : { x: node.position.x, y: node.position.y };
}

function measuredSizeOf(node: WorkflowNode): { width: number; height: number } {
  const flow = findNode(node.id);
  const width = flow?.dimensions?.width ?? 0;
  const height = flow?.dimensions?.height ?? 0;
  return {
    width: width > 0 ? width : NODE_WIDTH,
    height: height > 0 ? height : NODE_HEIGHT,
  };
}

/** 依据子节点（含嵌套分组）实际位置计算所有分组容器的边界。 */
function computeGroupBoxes(): Map<string, GroupBox> {
  const draft = store.draft;
  const result = new Map<string, GroupBox>();
  if (!draft) return result;

  const groups = draft.nodes.filter((node) => node.kind === "group");
  const groupIds = new Set(groups.map((group) => group.id));
  const childrenOf = new Map<string, WorkflowNode[]>();
  for (const node of draft.nodes) {
    const parentId = node.parentId;
    if (!parentId || parentId === node.id || !groupIds.has(parentId)) continue;
    const bucket = childrenOf.get(parentId);
    if (bucket) bucket.push(node);
    else childrenOf.set(parentId, [node]);
  }

  const resolving = new Set<string>();

  function resolve(group: WorkflowNode): GroupBox {
    const cached = result.get(group.id);
    if (cached) return cached;
    const stored: GroupBox = {
      x: Math.round(group.position.x),
      y: Math.round(group.position.y),
      width: DEFAULT_GROUP_WIDTH,
      height: DEFAULT_GROUP_HEIGHT,
    };
    // parentId 成环（非法结构）时停止递归，避免死循环。
    if (resolving.has(group.id)) return stored;
    resolving.add(group.id);

    const children = childrenOf.get(group.id) ?? [];
    if (children.length === 0) {
      resolving.delete(group.id);
      result.set(group.id, stored);
      return stored;
    }

    let minX = Number.POSITIVE_INFINITY;
    let minY = Number.POSITIVE_INFINITY;
    let maxX = Number.NEGATIVE_INFINITY;
    let maxY = Number.NEGATIVE_INFINITY;

    for (const child of children) {
      const box =
        child.kind === "group"
          ? resolve(child)
          : (() => {
              const position = flowPositionOf(child);
              const size = measuredSizeOf(child);
              return {
                x: position.x,
                y: position.y,
                width: size.width,
                height: size.height,
              };
            })();
      minX = Math.min(minX, box.x);
      minY = Math.min(minY, box.y);
      maxX = Math.max(maxX, box.x + box.width);
      maxY = Math.max(maxY, box.y + box.height);
    }

    const box: GroupBox = {
      x: Math.round(minX - GROUP_PADDING),
      y: Math.round(minY - GROUP_HEADER - GROUP_PADDING),
      width: Math.round(maxX - minX + GROUP_PADDING * 2),
      height: Math.round(maxY - minY + GROUP_HEADER + GROUP_PADDING * 2),
    };
    resolving.delete(group.id);
    result.set(group.id, box);
    return box;
  }

  for (const group of groups) resolve(group);
  return result;
}

/** 把分组边界同步到 Vue Flow 节点（位置 / 尺寸，仅 store，不写 draft）。 */
function updateGroupGeometry() {
  const draft = store.draft;
  if (!draft) return;
  const boxes = computeGroupBoxes();
  groupBoxes.value = boxes;

  for (const group of draft.nodes) {
    if (group.kind !== "group") continue;
    // 正在被拖动的那一组由拖拽逻辑控制位置，这里不要抢。
    if (draggingGroupId.value === group.id) continue;
    const box = boxes.get(group.id);
    if (!box) continue;
    const current = findNode(group.id);
    const unchanged =
      current &&
      Math.round(current.position.x) === box.x &&
      Math.round(current.position.y) === box.y &&
      Math.round(current.dimensions.width) === box.width &&
      Math.round(current.dimensions.height) === box.height;
    if (unchanged) continue;
    updateNode(group.id, {
      position: { x: box.x, y: box.y },
      width: box.width,
      height: box.height,
      style: {
        width: `${box.width}px`,
        height: `${box.height}px`,
      },
    });
  }
}

/** 把分组边界回写到 draft（拖动结束 / 自动排列 / 结构变化后调用）。 */
function persistGroupPositions() {
  const draft = store.draft;
  if (!draft) return;
  for (const group of draft.nodes) {
    if (group.kind !== "group") continue;
    const box = groupBoxes.value.get(group.id);
    if (!box) continue;
    if (
      Math.round(group.position.x) === box.x &&
      Math.round(group.position.y) === box.y
    ) {
      continue;
    }
    group.position = { x: box.x, y: box.y };
  }
}

/** 拖动子节点等高频场景：按帧合并，避免每个 mousemove 都写 store。 */
function scheduleGroupGeometry() {
  if (unmounted || geometryFrame) return;
  geometryFrame = window.requestAnimationFrame(() => {
    geometryFrame = 0;
    if (unmounted) return;
    updateGroupGeometry();
  });
}

/** 在分组现有边界内按「分组布局计划」重新摆放其直接子项，嵌套分组递归处理。 */
function layoutChildrenOf(
  group: WorkflowNode,
  origin?: { x: number; y: number },
) {
  const draft = store.draft;
  if (!draft) return;
  const box = origin ?? groupBoxes.value.get(group.id);
  const left = box?.x ?? Math.round(group.position.x);
  const top = box?.y ?? Math.round(group.position.y);

  // 连线药丸不参与分组内的常规排布：布局图里先折叠掉，最后按两端中点摆放。
  const pills = computeDelayPills(draft.nodes, draft.edges);
  const layoutGraph = buildDelayLayoutGraph(draft.nodes, draft.edges, pills);
  const plan = computeGroupPlans(layoutGraph.nodes, layoutGraph.edges).get(
    group.id,
  );

  if (plan) {
    for (const [childId, relative] of plan.children) {
      const child = draft.nodes.find((node) => node.id === childId);
      if (!child) continue;
      const position: NodePosition = {
        x: Math.round(left + relative.x),
        y: Math.round(top + relative.y),
      };
      if (child.position.x !== position.x || child.position.y !== position.y) {
        child.position = position;
      }
      updateNode(childId, { position });
    }
  }

  // 嵌套分组：子分组的内部节点跟随新位置重排（否则会留在旧坐标导致容器异常膨大）。
  for (const child of draft.nodes) {
    if (child.parentId !== group.id || child.kind !== "group") continue;
    layoutChildrenOf(child, {
      x: Math.round(child.position.x),
      y: Math.round(child.position.y),
    });
  }

  // 组内的连线药丸：按 source/target 当前位置取中点。
  const pillChildren = draft.nodes.filter(
    (node) => node.parentId === group.id && pills.has(node.id),
  );
  if (pillChildren.length > 0) {
    const positions = new Map<string, NodePosition>();
    for (const node of draft.nodes) {
      positions.set(node.id, { x: node.position.x, y: node.position.y });
    }
    const pillSubset = new Map(
      [...pills].filter(([id]) =>
        pillChildren.some((child) => child.id === id),
      ),
    );
    placeDelayPills(pillSubset, positions);
    for (const child of pillChildren) {
      const position = positions.get(child.id);
      if (!position) continue;
      if (child.position.x !== position.x || child.position.y !== position.y) {
        child.position = position;
      }
      updateNode(child.id, { position });
    }
  }
}

/**
 * 载入/切换工作流时建立结构快照（不做任何 draft 写入，避免一打开就变成「未保存」），
 * 只把派生出的分组边界同步到画布。
 */
function primeGroupState() {
  const draft = store.draft;
  if (!draft) return;

  lastGroupSignature.clear();
  lastParentOf.clear();

  const groups = draft.nodes.filter((node) => node.kind === "group");
  const groupIds = new Set(groups.map((group) => group.id));
  for (const group of groups) {
    const children = draft.nodes.filter((node) => node.parentId === group.id);
    lastGroupSignature.set(
      group.id,
      `${group.groupParallel ? "p" : "s"}:${children
        .map((child) => child.id)
        .join(",")}`,
    );
  }
  for (const node of draft.nodes) {
    const parent =
      node.parentId && node.parentId !== node.id && groupIds.has(node.parentId)
        ? node.parentId
        : null;
    lastParentOf.set(node.id, parent);
  }

  updateGroupGeometry();
}

/**
 * 结构变化（增删节点、切换分组归属、并行开关）后：
 * 1. 新加入分组的节点 / 并行开关变化的组，在组内重新摆位；
 * 2. 重算并持久化分组边界。
 */
function syncGroupStructure() {
  const draft = store.draft;
  if (!draft) return;

  const groups = draft.nodes.filter((node) => node.kind === "group");
  const groupIds = new Set(groups.map((group) => group.id));

  for (const group of groups) {
    const children = draft.nodes.filter((node) => node.parentId === group.id);
    const signature = `${group.groupParallel ? "p" : "s"}:${children
      .map((child) => child.id)
      .join(",")}`;
    const prev = lastGroupSignature.get(group.id);
    if (prev !== undefined && prev !== signature) {
      layoutChildrenOf(group);
    }
    lastGroupSignature.set(group.id, signature);
  }
  for (const id of Array.from(lastGroupSignature.keys())) {
    if (!groupIds.has(id)) lastGroupSignature.delete(id);
  }

  for (const node of draft.nodes) {
    const prev = lastParentOf.get(node.id) ?? null;
    const next =
      node.parentId && node.parentId !== node.id && groupIds.has(node.parentId)
        ? node.parentId
        : null;
    lastParentOf.set(node.id, next);
    if (prev === next) continue;
    if (next) {
      const group = draft.nodes.find((item) => item.id === next);
      if (group) layoutChildrenOf(group);
    }
  }

  updateGroupGeometry();
  persistGroupPositions();
}

/** 上一次重建时的 draft id，用于判断是否需要 fitView。 */
let lastDraftId: string | null = null;
let unmounted = false;

// ---------------------------------------------------------------------------
// 节点右键菜单（单点执行）
// ---------------------------------------------------------------------------
interface NodeContextMenuState {
  x: number;
  y: number;
  nodeId: string;
}

const contextMenu = ref<NodeContextMenuState | null>(null);
const menuRef = ref<HTMLElement | null>(null);
const runningNodeId = ref<string | null>(null);

const menuNode = computed(() => {
  const id = contextMenu.value?.nodeId;
  if (!id) return null;
  return store.draft?.nodes.find((node) => node.id === id) ?? null;
});

const menuDisabled = computed(
  () =>
    !menuNode.value ||
    menuNode.value.kind === "group" ||
    runningNodeId.value !== null,
);

const menuTitle = computed(() =>
  menuNode.value?.kind === "group"
    ? t("node.groupHint")
    : t("workflow.runNodeHint"),
);

function openNodeContextMenu(event: NodeMouseEvent) {
  const mouse = event.event as MouseEvent;
  if (typeof mouse.clientX !== "number" || typeof mouse.clientY !== "number") {
    return;
  }
  mouse.preventDefault();
  const node = store.draft?.nodes.find((item) => item.id === event.node.id);
  if (!node) return;
  contextMenu.value = {
    x: Math.min(mouse.clientX, Math.max(0, window.innerWidth - 220)),
    y: Math.min(mouse.clientY, Math.max(0, window.innerHeight - 120)),
    nodeId: node.id,
  };
}

function closeContextMenu() {
  contextMenu.value = null;
}

function onWindowPointerDown(event: PointerEvent) {
  const menu = menuRef.value;
  if (menu && event.target instanceof Node && menu.contains(event.target)) {
    return;
  }
  closeContextMenu();
}

function onWindowKeydown(event: KeyboardEvent) {
  if (event.key === "Escape") closeContextMenu();
}

watch(contextMenu, (menu) => {
  if (menu) {
    window.addEventListener("pointerdown", onWindowPointerDown, true);
    window.addEventListener("keydown", onWindowKeydown);
  } else {
    window.removeEventListener("pointerdown", onWindowPointerDown, true);
    window.removeEventListener("keydown", onWindowKeydown);
  }
});

async function runNodeNow() {
  const menu = contextMenu.value;
  const current = store.draft;
  closeContextMenu();
  if (!menu || !current) return;
  const node = current.nodes.find((item) => item.id === menu.nodeId);
  if (!node || node.kind === "group") return;
  if (runningNodeId.value !== null) return;

  runningNodeId.value = node.id;
  try {
    const run = await runSingleNode(current, node.id);
    store.applyRun(run);
    ui.notify(
      "success",
      t("workflow.runNodeStarted", {
        name: node.name || t(nodeKindMeta[node.kind].labelKey),
      }),
    );
  } catch (error) {
    ui.notify("error", t("workflow.runNodeFailed", { error: errorText(error) }));
  } finally {
    runningNodeId.value = null;
  }
}

// ---------------------------------------------------------------------------
// 拖放 .lnk / .exe 等文件到画布创建 application 节点
// ---------------------------------------------------------------------------
const DROP_EXTENSIONS = new Set(["lnk", "exe", "bat", "cmd", "ps1", "vbs"]);
let dragUnlisten: UnlistenFn | null = null;
let dragDisposed = false;

async function handleFileDrop(
  paths: string[],
  position: { x: number; y: number },
) {
  const current = store.draft;
  if (!current) return;

  const supported = paths.filter((path) =>
    DROP_EXTENSIONS.has(fileExtension(path)),
  );
  if (supported.length === 0) {
    if (paths.length > 0) ui.notify("warn", t("workflow.dropUnsupported"));
    return;
  }

  // Tauri 给的是相对 webview 左上角的物理像素，先换算成 CSS 像素再交给 Vue Flow。
  const scale = window.devicePixelRatio || 1;
  const base = screenToFlowCoordinate({
    x: position.x / scale,
    y: position.y / scale,
  });
  const dropX = Number.isFinite(base.x) ? base.x : 0;
  const dropY = Number.isFinite(base.y) ? base.y : 0;

  const created: WorkflowNode[] = [];
  supported.forEach((path, index) => {
    const offset = index * 24;
    const node = createWorkflowNode("application", {
      x: Math.round(dropX + offset - NODE_WIDTH / 2),
      y: Math.round(dropY + offset - 34),
    });
    node.name = fileBaseName(path);
    node.app.path = path;
    node.app.args = [];
    current.nodes.push(node);
    created.push(node);
  });

  addNodes(created.map((node) => buildFlowNode(node)));
  const last = created[created.length - 1];
  if (last) {
    void nextTick(() => {
      const added = findNode(last.id);
      if (added) addSelectedNodes([added]);
    });
  }

  ui.notify("success", t("workflow.dropCreated", { count: created.length }));
  if (supported.length < paths.length) {
    ui.notify("warn", t("workflow.dropUnsupported"));
  }
}

onMounted(async () => {
  try {
    const unlisten = await getCurrentWebview().onDragDropEvent((event) => {
      if (event.payload.type !== "drop") return;
      void handleFileDrop(event.payload.paths, event.payload.position);
    });
    if (dragDisposed) {
      unlisten();
      return;
    }
    dragUnlisten = unlisten;
  } catch {
    // 浏览器预览等非 Tauri 环境没有该事件，静默忽略。
  }
});

onBeforeUnmount(() => {
  unmounted = true;
  dragDisposed = true;
  dragUnlisten?.();
  dragUnlisten = null;
  window.removeEventListener("pointerdown", onWindowPointerDown, true);
  window.removeEventListener("keydown", onWindowKeydown);
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
  if (suppressSelectionEmit) return;
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
  // 撤销/重做会把画布整体重建，此时外部（属性面板）仍保持着选中节点，这里一并恢复。
  if (
    props.selectedNodeId &&
    !selectedNodeIds.includes(props.selectedNodeId)
  ) {
    selectedNodeIds.push(props.selectedNodeId);
  }
  const selectedEdgeIds = getSelectedEdges.value.map((edge) => edge.id);

  lastGroupSignature.clear();
  lastParentOf.clear();
  groupBoxes.value = new Map();

  // 分组节点必须先创建并带上尺寸：Vue Flow 需要量测到尺寸才会显示节点，
  // 若是 0×0 就会一直 visibility:hidden，容器背景不可见。
  const initialBoxes = computeGroupBoxes();
  const pills = computeDelayPills(next.nodes, next.edges);
  const orderedNodes = [
    ...next.nodes.filter((node) => node.kind === "group"),
    ...next.nodes.filter((node) => node.kind !== "group"),
  ];
  flowNodes.value = orderedNodes.map((node) =>
    buildFlowNode(node, initialBoxes.get(node.id), pills.has(node.id)),
  );
  applyTopology(next);
  flowEdges.value = buildEdgesFor(next);

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

    // 建立结构快照 + 计算分组容器边界（首次量测后还会再刷新一次）。
    primeGroupState();
    scheduleGroupGeometry();

    if (idChanged && next.nodes.length > 0) {
      fitWhenReady();
    }
    emitSelection();
  });
}

// draft 只在 loadDraft / saveDraft / createDraft / discardDraft 时整体替换，
// 字段级编辑通过 data.node 引用直接反映，不需要重建画布（也就不会打断拖拽）。
watch(() => store.draft, (next) => rebuildFromDraft(next), { immediate: true });

// 节点类型切换：kind 就是画布节点 type，直接就地更新 store 中的节点；
// 同时检测分组归属 / 并行开关变化，触发组内重排与容器边界重算。
watch(
  () =>
    (store.draft?.nodes ?? [])
      .map(
        (node) =>
          `${node.id}:${node.kind}:${node.enabled ? "1" : "0"}:${
            node.parentId ?? "-"
          }:${node.kind === "group" ? (node.groupParallel ? "p" : "s") : ""}`,
      )
      .join("|"),
  () => {
    if (!store.draft) return;
    syncGroupStructure();
    // refreshFlowEdges 内部会同步节点类型（含连线药丸的折叠/还原）。
    refreshFlowEdges();
  },
);

// 连线增删 / 条件变化：重算执行流与依赖跳线的分类。
watch(
  () =>
    (store.draft?.edges ?? [])
      .map((edge) => `${edge.id}:${edge.source}:${edge.target}:${edge.condition}`)
      .join("|"),
  () => refreshFlowEdges(),
);

// 主题切换时重算连线箭头颜色。
watch(
  () => theme.isDark,
  () => {
    if (!store.draft) return;
    flowEdges.value = buildEdgesFor(store.draft);
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
  // 拓扑变化后统一重算步骤号 + 连线分类（新边可能是执行流也可能是跳线）。
  refreshFlowEdges();
}

/** 点击连线：若该边两端折叠了延时药丸，则改为选中延时节点，右栏可直接改延时。 */
function onEdgeClick(event: EdgeMouseEvent) {
  const current = store.draft;
  if (!current) return;
  const pills = computeDelayPills(current.nodes, current.edges);
  const delayId = pillByEdge(pills).get(event.edge.id);
  if (!delayId) return;
  // Vue Flow 会在 click 内先选中边，这里下一拍改成选中节点。
  void nextTick(() => {
    const node = findNode(delayId);
    if (!node) return;
    removeSelectedElements();
    addSelectedNodes([node]);
    emitSelection();
  });
}

/** Vue Flow 内置删除（Delete / Backspace）只会改内部 state，这里同步回 draft。 */
function onNodesChange(changes: NodeChange[]) {
  const current = store.draft;
  if (!current) return;
  const removedIds = changes
    .filter((change): change is NodeRemoveChange => change.type === "remove")
    .map((change) => change.id);
  if (removedIds.length === 0) return;
  // 删除分组时只解绑子项，不连带删除子节点。
  removedIds.forEach((id) => unbindGroupChildren(id));
  current.nodes = current.nodes.filter((node) => !removedIds.includes(node.id));
  current.edges = current.edges.filter(
    (edge) => !removedIds.includes(edge.source) && !removedIds.includes(edge.target),
  );
  scheduleGroupGeometry();
  void nextTick(() => persistGroupPositions());
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

function onNodeDragStart(event: NodeDragEvent) {
  if (event.node.type !== "group") {
    groupDragState = null;
    draggingGroupId.value = null;
    return;
  }
  const flow = findNode(event.node.id);
  // 以「取整后的分组位置」为基准，保证后续每次应用到后代上的 delta 都是整数。
  const last = flow
    ? { x: Math.round(flow.position.x), y: Math.round(flow.position.y) }
    : {
        x: Math.round(event.node.position.x),
        y: Math.round(event.node.position.y),
      };
  groupDragState = {
    groupId: event.node.id,
    last,
    subtree: collectSubtreeIds(event.node.id),
  };
  draggingGroupId.value = event.node.id;
}

/** 拖动过程中：分组整体平移（子节点跟随），子节点拖动则只收敛分组边界。 */
function onNodeDrag(event: NodeDragEvent) {
  if (groupDragState && event.node.id === groupDragState.groupId) {
    const groupFlow = findNode(groupDragState.groupId);
    if (!groupFlow) return;
    // 分组视觉位置可能带小数（缩放/吸附），只取整数 delta 应用到后代，
    // 余量留到下一帧或拖动结束再补齐，保证相对位置不被抹成小数。
    const targetX = Math.round(groupFlow.position.x);
    const targetY = Math.round(groupFlow.position.y);
    const deltaX = targetX - groupDragState.last.x;
    const deltaY = targetY - groupDragState.last.y;
    if (deltaX !== 0 || deltaY !== 0) {
      for (const id of groupDragState.subtree) {
        const child = findNode(id);
        if (!child) continue;
        updateNode(id, {
          position: {
            x: Math.round(child.position.x + deltaX),
            y: Math.round(child.position.y + deltaY),
          },
        });
      }
      groupDragState.last = { x: targetX, y: targetY };
      // 嵌套分组的边界要跟着平移后的子项重新收敛。
      scheduleGroupGeometry();
    }
    return;
  }
  if (event.nodes.some((node) => node.type !== "group")) {
    scheduleGroupGeometry();
  }
}

function onNodeDragStop(event: NodeDragEvent) {
  const current = store.draft;
  if (!current) return;

  const draggedGroups = event.nodes.filter((node) => node.type === "group");
  if (draggedGroups.length > 0) {
    // 收尾：把拖动过程中剩余的整数 delta 也应用到后代，保证分组与子项同步。
    if (groupDragState) {
      const groupFlow = findNode(groupDragState.groupId);
      if (groupFlow) {
        const deltaX = Math.round(groupFlow.position.x) - groupDragState.last.x;
        const deltaY = Math.round(groupFlow.position.y) - groupDragState.last.y;
        if (deltaX !== 0 || deltaY !== 0) {
          for (const id of groupDragState.subtree) {
            const child = findNode(id);
            if (!child) continue;
            updateNode(id, {
              position: {
                x: Math.round(child.position.x + deltaX),
                y: Math.round(child.position.y + deltaY),
              },
            });
          }
        }
      }
    }

    // 整组拖动：分组本身 + 所有后代的位置一起写回 draft（只产生一条撤销记录）。
    const ids = new Set<string>();
    for (const group of draggedGroups) {
      ids.add(group.id);
      for (const id of collectSubtreeIds(group.id)) ids.add(id);
    }
    for (const id of ids) {
      const flow = findNode(id);
      const node = current.nodes.find((item) => item.id === id);
      if (!flow || !node) continue;
      node.position = {
        x: Math.round(flow.position.x),
        y: Math.round(flow.position.y),
      };
    }
    groupDragState = null;
    draggingGroupId.value = null;
    updateGroupGeometry();
    persistGroupPositions();
    return;
  }

  for (const dragged of event.nodes) {
    const node = current.nodes.find((item) => item.id === dragged.id);
    if (!node) continue;
    if (node.kind === "group") continue;
    node.position = {
      x: Math.round(dragged.position.x),
      y: Math.round(dragged.position.y),
    };
  }
  updateGroupGeometry();
  persistGroupPositions();
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
  node.name =
    kind === "group" ? `新分组 ${index}` : `${t(meta.labelKey)} ${index}`;
  current.nodes.push(node);
  addNodes(buildFlowNode(node));
  if (kind === "group") {
    scheduleGroupGeometry();
    void nextTick(() => persistGroupPositions());
  }
  void nextTick(() => {
    const added = findNode(node.id);
    if (added) addSelectedNodes([added]);
  });
}

/** 节点面板点击：启动应用走「开始菜单选择器」，其余类型直接落节点。 */
function onPaletteClick(kind: NodeKind) {
  if (kind === "application") {
    emit("pick-app");
    return;
  }
  addNode(kind);
}

/** 开始菜单选择器选中后创建 application 节点（名称/路径/参数直接来自快捷方式）。 */
function addApplicationNode(app: StartMenuApp) {
  const current = store.draft;
  if (!current) return;
  const node = createWorkflowNode("application", nextNodePosition());
  node.name = app.name;
  node.app.path = app.path;
  node.app.args = [...app.args];
  current.nodes.push(node);
  addNodes(buildFlowNode(node));
  void nextTick(() => {
    const added = findNode(node.id);
    if (added) addSelectedNodes([added]);
  });
}

/** 删除分组时把直接子项的 parentId 置空（子节点保留）。 */
function unbindGroupChildren(groupId: string) {
  const current = store.draft;
  if (!current) return;
  const group = current.nodes.find((node) => node.id === groupId);
  if (!group || group.kind !== "group") return;
  for (const node of current.nodes) {
    if (node.parentId === groupId) node.parentId = null;
  }
  lastGroupSignature.delete(groupId);
}

function removeNodeById(id: string) {
  const current = store.draft;
  if (current) {
    unbindGroupChildren(id);
    current.nodes = current.nodes.filter((node) => node.id !== id);
    current.edges = current.edges.filter(
      (edge) => edge.source !== id && edge.target !== id,
    );
    lastParentOf.delete(id);
  }
  removeNodes([id]);
  scheduleGroupGeometry();
  void nextTick(() => persistGroupPositions());
}

function removeEdgeById(id: string) {
  const current = store.draft;
  if (current) {
    current.edges = current.edges.filter((edge) => edge.id !== id);
  }
  removeEdges([id]);
}

/** 自动排列（landscape 思维导图）：按拓扑展开、同层水平排开，写回 draft（可撤销）。 */
function autoLayout() {
  const current = store.draft;
  if (!current || current.nodes.length === 0) return;

  const positions = computeAutoLayout(current.nodes, current.edges);
  for (const node of current.nodes) {
    const position = positions.get(node.id);
    if (!position) continue;
    node.position = position;
    updateNode(node.id, { position });
  }

  void nextTick(() => {
    // 分组容器以子节点实际位置为准，这里再收敛一次并持久化。
    updateGroupGeometry();
    persistGroupPositions();
    fitWhenReady(2);
  });
}

function fitViewToContent() {
  fitWhenReady(2);
}

defineExpose({
  addNode,
  addApplicationNode,
  removeNodeById,
  removeEdgeById,
  autoLayout,
  fitViewToContent,
});
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
      :elevate-nodes-on-select="false"
      :edges-updatable="false"
      :snap-to-grid="snapEnabled"
      :snap-grid="SNAP_GRID"
      :default-marker-color="theme.isDark ? '#818cf8' : '#6366f1'"
      @init="onInit"
      @connect="onConnect"
      @nodes-change="onNodesChange"
      @edges-change="onEdgesChange"
      @edge-click="onEdgeClick"
      @node-drag-start="onNodeDragStart"
      @node-drag="onNodeDrag"
      @node-drag-stop="onNodeDragStop"
      @node-context-menu="openNodeContextMenu"
      @nodes-initialized="scheduleGroupGeometry"
    >
      <template #node-application="nodeProps">
        <WorkflowNodeCard v-bind="nodeProps" />
      </template>
      <template #node-delay="nodeProps">
        <WorkflowNodeCard v-bind="nodeProps" />
      </template>
      <template #node-delay-pill="nodeProps">
        <WorkflowDelayPill v-bind="nodeProps" />
      </template>
      <template #node-wait="nodeProps">
        <WorkflowNodeCard v-bind="nodeProps" />
      </template>
      <template #node-script="nodeProps">
        <WorkflowNodeCard v-bind="nodeProps" />
      </template>
      <template #node-group="nodeProps">
        <WorkflowGroupNode v-bind="nodeProps" />
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
            class="flex gap-1.5 rounded-lg border border-transparent px-1.5 py-1.5 text-left transition hover:border-zinc-200 hover:bg-zinc-50 dark:hover:border-zinc-700 dark:hover:bg-zinc-800/60"
            :class="
              kind === 'group'
                ? 'col-span-2 items-center border-dashed border-teal-300/70 dark:border-teal-500/30'
                : 'flex-col items-start'
            "
            :title="t(nodeKindMeta[kind].hintKey)"
            @click="onPaletteClick(kind)"
          >
            <span
              class="flex h-6 w-6 shrink-0 items-center justify-center rounded-md"
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
          :class="
            snapEnabled
              ? 'text-brand-600 dark:text-brand-400'
              : 'text-zinc-400 dark:text-zinc-500'
          "
          :title="`${snapLabel} · ${snapHint}`"
          :aria-pressed="snapEnabled"
          @click="toggleSnap"
        >
          <Magnet class="h-3.5 w-3.5" />
        </BaseButton>
        <span class="mx-0.5 h-4 w-px bg-zinc-200 dark:bg-zinc-700" />
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

      <!-- 连线图例 -->
      <Panel
        position="bottom-right"
        class="flex flex-col gap-1 rounded-lg border border-zinc-200 bg-white/95 px-2 py-1.5 text-[10px] text-zinc-500 shadow-sm backdrop-blur dark:border-zinc-800 dark:bg-zinc-900/95 dark:text-zinc-400"
      >
        <span class="inline-flex items-center gap-1.5">
          <span class="h-0.5 w-5 shrink-0 rounded-full bg-brand-500" />
          {{ t("workflow.edgeFlow") }}
        </span>
        <span class="inline-flex items-center gap-1.5">
          <span
            class="h-0 w-5 shrink-0 border-t-2 border-dashed border-zinc-400 dark:border-zinc-500"
          />
          {{ t("workflow.edgeDependency") }}
        </span>
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
        <p class="mt-1 text-center text-[11px] text-zinc-400 dark:text-zinc-500">
          {{ t("workflow.dragHint") }}
        </p>
      </div>
    </div>

    <!-- 节点右键菜单：单点执行 -->
    <Teleport to="body">
      <div
        v-if="contextMenu"
        ref="menuRef"
        class="fixed z-[70] w-52 overflow-hidden rounded-lg border border-zinc-200 bg-white py-1 shadow-xl dark:border-zinc-700 dark:bg-zinc-900"
        :style="{ left: `${contextMenu.x}px`, top: `${contextMenu.y}px` }"
      >
        <button
          type="button"
          class="flex w-full items-center gap-2 px-3 py-1.5 text-left text-xs transition"
          :class="
            menuDisabled
              ? 'cursor-not-allowed text-zinc-400 dark:text-zinc-600'
              : 'text-zinc-700 hover:bg-zinc-100 dark:text-zinc-200 dark:hover:bg-zinc-800'
          "
          :disabled="menuDisabled"
          :title="menuTitle"
          @click="runNodeNow"
        >
          <Play class="h-3.5 w-3.5 shrink-0" />
          <span class="min-w-0 flex-1 truncate">{{ t("workflow.runNode") }}</span>
          <span
            v-if="runningNodeId === contextMenu.nodeId"
            class="h-1.5 w-1.5 shrink-0 animate-pulse rounded-full bg-brand-500"
          />
        </button>
      </div>
    </Teleport>
  </div>
</template>
