<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import {
  AlertTriangle,
  GitBranch,
  Play,
  Plus,
  Save,
  ShieldAlert,
  Square,
  XCircle,
} from "lucide-vue-next";

import type {
  NodePosition,
  StartMenuApp,
  WorkflowValidation,
} from "@/api";
import PageHeader from "@/modules/components/layout/PageHeader.vue";
import BaseBadge from "@/modules/components/ui/BaseBadge.vue";
import BaseButton from "@/modules/components/ui/BaseButton.vue";
import BaseModal from "@/modules/components/ui/BaseModal.vue";
import BaseSpinner from "@/modules/components/ui/BaseSpinner.vue";
import EmptyState from "@/modules/components/ui/EmptyState.vue";
import { useI18n } from "@/modules/locales";
import { useIconStore } from "@/modules/stores/icons";
import { useUiStore } from "@/modules/stores/ui";
import { useWorkflowStore } from "@/modules/stores/workflows";

import StartMenuPicker from "@/modules/components/workflow/StartMenuPicker.vue";
import WorkflowCanvas from "@/modules/components/workflow/WorkflowCanvas.vue";
import WorkflowInspector from "@/modules/components/workflow/WorkflowInspector.vue";
import WorkflowListPanel from "@/modules/components/workflow/WorkflowListPanel.vue";
import WorkflowRunHistory from "@/modules/components/workflow/WorkflowRunHistory.vue";
import WorkflowTextView from "@/modules/components/workflow/WorkflowTextView.vue";
import WorkflowToolbar from "@/modules/components/workflow/WorkflowToolbar.vue";
import { errorText } from "@/modules/components/workflow/shared";
import { createWorkflowNode } from "@/modules/components/workflow/factory";
import type {
  WorkflowSelection,
  WorkflowViewMode,
} from "@/modules/components/workflow/types";

const store = useWorkflowStore();
const ui = useUiStore();
const icons = useIconStore();
const { t } = useI18n();

const canvasRef = ref<InstanceType<typeof WorkflowCanvas> | null>(null);

const selectedNodeId = ref<string | null>(null);
const selectedEdgeId = ref<string | null>(null);

/** 节点画布 / 高密度文本视图。 */
const viewMode = ref<WorkflowViewMode>("nodes");

/** 开始菜单选择器：create = 新建应用节点，replace = 替换现有应用节点路径。 */
const pickerOpen = ref(false);
const pickerMode = ref<"create" | "replace">("create");
const pickerNodeId = ref<string | null>(null);

const validationOpen = ref(false);
const validation = ref<WorkflowValidation | null>(null);

const draft = computed(() => store.draft);
const draftRunning = computed(() => store.draftRun?.status === "running");

/** 当前工作流最近 10 次运行。 */
const workflowRuns = computed(() => {
  const id = draft.value?.id;
  if (!id) return [];
  return store.runs.filter((run) => run.workflowId === id).slice(0, 10);
});

const emptyTitle = computed(() =>
  store.workflows.length === 0 ? t("workflow.empty") : t("workflow.selectHint"),
);

const emptyDescription = computed(() =>
  store.workflows.length === 0
    ? t("workflow.emptyHint")
    : t("workflow.selectHintDesc"),
);

function onSelectionChange(selection: WorkflowSelection) {
  selectedNodeId.value = selection.nodeId;
  selectedEdgeId.value = selection.edgeId;
}

/** 文本视图里点某一行：选中节点并让右侧属性面板联动。 */
function onSelectNode(nodeId: string) {
  selectedNodeId.value = nodeId;
  selectedEdgeId.value = null;
}

function setViewMode(next: WorkflowViewMode) {
  viewMode.value = next;
  // 文本视图没有「选中边」的概念，避免切过去后右栏还停留在边的属性上。
  if (next === "text") selectedEdgeId.value = null;
}

/** 打开开始菜单选择器：create = 新建应用节点，replace = 替换当前应用节点路径。 */
function openAppPicker(mode: "create" | "replace") {
  if (mode === "replace" && !selectedNodeId.value) return;
  pickerMode.value = mode;
  pickerNodeId.value = mode === "replace" ? selectedNodeId.value : null;
  pickerOpen.value = true;
}

/** 画布卸载（文本视图）时的兜底位置。 */
function fallbackPosition(): NodePosition {
  const offset = (store.draft?.nodes.length ?? 0) % 4;
  return { x: 120 + offset * 40, y: 120 + offset * 40 };
}

function createApplicationNode(app: StartMenuApp | null) {
  const current = store.draft;
  if (!current) return;
  const node = createWorkflowNode("application", fallbackPosition());
  if (app) {
    node.name = app.name;
    node.app.path = app.path;
    node.app.args = [...app.args];
  } else {
    const index =
      current.nodes.filter((item) => item.kind === "application").length + 1;
    node.name = `${t("node.kind.application")} ${index}`;
  }
  current.nodes.push(node);
  selectedNodeId.value = node.id;
  selectedEdgeId.value = null;
}

function onAppPicked(app: StartMenuApp) {
  pickerOpen.value = false;
  if (pickerMode.value === "create") {
    const canvas = canvasRef.value;
    if (canvas) canvas.addApplicationNode(app);
    else createApplicationNode(app);
    return;
  }
  const node = store.draft?.nodes.find((item) => item.id === pickerNodeId.value);
  if (!node) return;
  node.name = app.name;
  node.app.path = app.path;
  node.app.args = [...app.args];
}

function onAppManual() {
  pickerOpen.value = false;
  if (pickerMode.value !== "create") return;
  const canvas = canvasRef.value;
  if (canvas) canvas.addNode("application");
  else createApplicationNode(null);
}

/** 撤销/重做/切换后，选中项可能已不存在。 */
function clearMissingSelection() {
  const current = store.draft;
  if (!current) {
    selectedNodeId.value = null;
    selectedEdgeId.value = null;
    return;
  }
  if (
    selectedNodeId.value &&
    !current.nodes.some((node) => node.id === selectedNodeId.value)
  ) {
    selectedNodeId.value = null;
  }
  if (
    selectedEdgeId.value &&
    !current.edges.some((edge) => edge.id === selectedEdgeId.value)
  ) {
    selectedEdgeId.value = null;
  }
}

/** 批量懒加载应用节点图标（卡片 / 文本视图都从 icon store 读取）。 */
function syncApplicationIcons() {
  const paths = (store.draft?.nodes ?? [])
    .filter((node) => node.kind === "application")
    .map((node) => node.app.path.trim())
    .filter((path) => path.length > 0);
  if (paths.length > 0) void icons.ensure(paths);
}

watch(
  () =>
    (store.draft?.nodes ?? [])
      .filter((node) => node.kind === "application")
      .map((node) => node.app.path)
      .join("|"),
  () => syncApplicationIcons(),
  { immediate: true },
);

watch(() => store.draft, () => clearMissingSelection());

function confirmDiscard(): boolean {
  if (!store.draftDirty || !store.draft) return true;
  return window.confirm(t("workflow.discardConfirm"));
}

async function selectWorkflow(id: string) {
  if (store.draft?.id === id) return;
  if (!confirmDiscard()) return;
  selectedNodeId.value = null;
  selectedEdgeId.value = null;
  try {
    await store.loadDraft(id);
  } catch (error) {
    ui.notify("error", t("error.invoke", { error: errorText(error) }));
  }
}

function createWorkflow() {
  if (!confirmDiscard()) return;
  selectedNodeId.value = null;
  selectedEdgeId.value = null;
  store.createDraft(t("workflow.newDefaultName"));
}

async function duplicateWorkflow(id: string) {
  if (!confirmDiscard()) return;
  try {
    const copy = await store.duplicate(id);
    ui.notify("success", t("workflow.duplicated"));
    selectedNodeId.value = null;
    selectedEdgeId.value = null;
    await store.loadDraft(copy.id);
  } catch (error) {
    ui.notify("error", t("error.invoke", { error: errorText(error) }));
  }
}

async function removeWorkflow(id: string) {
  try {
    await store.remove(id);
    ui.notify("success", t("workflow.deleted"));
  } catch (error) {
    ui.notify("error", t("error.invoke", { error: errorText(error) }));
  }
}

async function saveWorkflow() {
  if (!store.draft) return;
  try {
    await store.saveDraft();
    ui.notify("success", t("workflow.saved"));
  } catch (error) {
    ui.notify("error", t("error.invoke", { error: errorText(error) }));
  }
}

async function validateDraft() {
  if (!store.draft) return;
  try {
    const result = await store.validateDraft();
    if (!result) return;
    validation.value = result;
    validationOpen.value = true;
    if (result.ok) {
      ui.notify("success", t("workflow.valid"));
    } else {
      ui.notify("error", t("workflow.invalid"));
    }
  } catch (error) {
    ui.notify("error", t("error.invoke", { error: errorText(error) }));
  }
}

async function runWorkflow() {
  const current = store.draft;
  if (!current) return;
  try {
    // run_workflow 读取的是已落库的配置，先把未保存的修改存下去。
    if (store.draftDirty) {
      const saved = await store.saveDraft();
      if (!saved) return;
    }
    const workflowId = store.draft?.id ?? current.id;
    const run = await store.run(workflowId);
    ui.notify(
      "success",
      t("workflow.runStarted", { name: run.workflowName || current.name }),
    );
  } catch (error) {
    ui.notify("error", `${t("workflow.runFailed")}：${errorText(error)}`);
  }
}

async function cancelRun() {
  const run = store.draftRun;
  if (!run) return;
  try {
    await store.cancel(run.runId);
    ui.notify("info", t("workflow.cancelled"));
  } catch (error) {
    ui.notify("error", t("error.invoke", { error: errorText(error) }));
  }
}

function onDeleteNode() {
  const id = selectedNodeId.value;
  if (!id) return;
  const canvas = canvasRef.value;
  if (canvas) {
    canvas.removeNodeById(id);
  } else if (store.draft) {
    // 文本视图下画布是卸载的，直接从 draft 删除；删除分组只解绑子项。
    const target = store.draft.nodes.find((node) => node.id === id);
    if (target?.kind === "group") {
      for (const node of store.draft.nodes) {
        if (node.parentId === id) node.parentId = null;
      }
    }
    store.draft.nodes = store.draft.nodes.filter((node) => node.id !== id);
    store.draft.edges = store.draft.edges.filter(
      (edge) => edge.source !== id && edge.target !== id,
    );
  }
  selectedNodeId.value = null;
}

function onDeleteEdge() {
  const id = selectedEdgeId.value;
  if (!id) return;
  const canvas = canvasRef.value;
  if (canvas) {
    canvas.removeEdgeById(id);
  } else if (store.draft) {
    store.draft.edges = store.draft.edges.filter((edge) => edge.id !== id);
  }
  selectedEdgeId.value = null;
}

/** Ctrl/Cmd+S 保存（输入框内也允许，符合常见编辑器习惯）。 */
/** 焦点在可编辑元素里时不拦截快捷键，交给原生撤销/输入。 */
function isEditableTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  const tag = target.tagName;
  return (
    tag === "INPUT" ||
    tag === "TEXTAREA" ||
    tag === "SELECT" ||
    target.isContentEditable ||
    Boolean(target.closest("[contenteditable='true'], .nokey"))
  );
}

function onKeydown(event: KeyboardEvent) {
  if (!(event.ctrlKey || event.metaKey)) return;
  const key = event.key.toLowerCase();

  if (key === "z") {
    if (isEditableTarget(event.target)) return;
    event.preventDefault();
    if (event.repeat || !store.draft) return;
    if (event.shiftKey) store.redo();
    else store.undo();
    return;
  }

  if (key === "y") {
    if (isEditableTarget(event.target)) return;
    event.preventDefault();
    if (event.repeat || !store.draft) return;
    store.redo();
    return;
  }

  if (key === "s") {
    event.preventDefault();
    if (!store.draft || event.repeat) return;
    void saveWorkflow();
  }
}

onMounted(async () => {
  window.addEventListener("keydown", onKeydown);
  try {
    await Promise.all([store.load(), store.loadRuns()]);
  } catch (error) {
    ui.notify("error", t("error.invoke", { error: errorText(error) }));
  }
});

onBeforeUnmount(() => {
  window.removeEventListener("keydown", onKeydown);
});
</script>

<template>
  <div class="flex h-full min-h-[560px] flex-col gap-3">
    <PageHeader :title="t('workflow.title')" :description="t('workflow.subtitle')">
      <template #actions>
        <BaseBadge v-if="store.draftDirty" variant="warn" dot>
          {{ t("workflow.unsaved") }}
        </BaseBadge>
        <BaseButton
          variant="secondary"
          size="sm"
          :disabled="!draft"
          :title="t('workflow.validate')"
          @click="validateDraft"
        >
          <ShieldAlert class="h-4 w-4" />
          {{ t("workflow.validate") }}
        </BaseButton>
        <BaseButton
          v-if="draftRunning"
          variant="danger"
          size="sm"
          @click="cancelRun"
        >
          <Square class="h-3.5 w-3.5" />
          {{ t("workflow.stop") }}
        </BaseButton>
        <BaseButton
          v-else
          variant="subtle"
          size="sm"
          :disabled="!draft"
          @click="runWorkflow"
        >
          <Play class="h-4 w-4" />
          {{ t("workflow.run") }}
        </BaseButton>
        <BaseButton
          variant="primary"
          size="sm"
          :disabled="!draft"
          :loading="store.saving"
          @click="saveWorkflow"
        >
          <Save v-if="!store.saving" class="h-4 w-4" />
          {{ t("workflow.save") }}
        </BaseButton>
      </template>
    </PageHeader>

    <div class="flex min-h-0 flex-1 gap-3">
      <WorkflowListPanel
        class="w-60 shrink-0 xl:w-64"
        :workflows="store.workflows"
        :selected-id="draft?.id ?? null"
        :loading="store.loading"
        @select="selectWorkflow"
        @create="createWorkflow"
        @duplicate="duplicateWorkflow"
        @remove="removeWorkflow"
      />

      <div class="flex min-h-0 min-w-0 flex-1 flex-col gap-3">
        <WorkflowToolbar
          v-if="draft"
          :draft="draft"
          :view="viewMode"
          @update:view="setViewMode"
        />

        <div
          class="relative flex min-h-[260px] flex-1 flex-col overflow-hidden rounded-xl border border-zinc-200 bg-white shadow-sm dark:border-zinc-800 dark:bg-zinc-950"
        >
          <WorkflowCanvas
            v-if="draft && viewMode === 'nodes'"
            :key="draft.id"
            ref="canvasRef"
            class="h-full flex-1"
            :selected-node-id="selectedNodeId"
            @selection-change="onSelectionChange"
            @pick-app="openAppPicker('create')"
          />
          <WorkflowTextView
            v-else-if="draft"
            class="h-full min-h-0 flex-1"
            :draft="draft"
            :selected-node-id="selectedNodeId"
            @select="onSelectNode"
          />
          <div v-else class="flex h-full items-center justify-center p-6">
            <BaseSpinner v-if="store.loading" size="lg" />
            <EmptyState v-else :title="emptyTitle" :description="emptyDescription">
              <template #icon>
                <GitBranch class="h-7 w-7" />
              </template>
              <template #action>
                <BaseButton variant="primary" size="sm" @click="createWorkflow">
                  <Plus class="h-3.5 w-3.5" />
                  {{ t("workflow.new") }}
                </BaseButton>
              </template>
            </EmptyState>
          </div>
        </div>

        <WorkflowRunHistory
          v-if="draft"
          class="h-40 shrink-0"
          :runs="workflowRuns"
        />
      </div>

      <WorkflowInspector
        class="w-72 shrink-0 xl:w-80"
        :node-id="selectedNodeId"
        :edge-id="selectedEdgeId"
        @delete-node="onDeleteNode"
        @delete-edge="onDeleteEdge"
        @pick-app="openAppPicker('replace')"
      />
    </div>

    <StartMenuPicker
      :open="pickerOpen"
      @close="pickerOpen = false"
      @pick="onAppPicked"
      @manual="onAppManual"
    />

    <BaseModal
      :open="validationOpen"
      :title="t('workflow.validate')"
      :description="validation?.ok ? t('workflow.valid') : t('workflow.invalid')"
      size="md"
      @close="validationOpen = false"
    >
      <div v-if="validation" class="space-y-4">
        <div v-if="validation.errors.length > 0" class="space-y-1.5">
          <p
            class="flex items-center gap-1.5 text-xs font-semibold text-red-600 dark:text-red-400"
          >
            <XCircle class="h-3.5 w-3.5" />
            {{ t("common.error") }} · {{ validation.errors.length }}
          </p>
          <ul class="space-y-1">
            <li
              v-for="(message, index) in validation.errors"
              :key="`error-${index}`"
              class="rounded-lg bg-red-50 px-2.5 py-1.5 text-xs leading-relaxed text-red-700 dark:bg-red-500/10 dark:text-red-300"
            >
              {{ message }}
            </li>
          </ul>
        </div>

        <div v-if="validation.warnings.length > 0" class="space-y-1.5">
          <p
            class="flex items-center gap-1.5 text-xs font-semibold text-amber-600 dark:text-amber-400"
          >
            <AlertTriangle class="h-3.5 w-3.5" />
            {{ t("logs.level.warn") }} · {{ validation.warnings.length }}
          </p>
          <ul class="space-y-1">
            <li
              v-for="(message, index) in validation.warnings"
              :key="`warning-${index}`"
              class="rounded-lg bg-amber-50 px-2.5 py-1.5 text-xs leading-relaxed text-amber-800 dark:bg-amber-500/10 dark:text-amber-200"
            >
              {{ message }}
            </li>
          </ul>
        </div>

        <p
          v-if="validation.ok && validation.warnings.length === 0"
          class="rounded-lg bg-emerald-50 px-2.5 py-2 text-xs text-emerald-700 dark:bg-emerald-500/10 dark:text-emerald-300"
        >
          {{ t("workflow.valid") }}
        </p>
      </div>

      <template #footer>
        <BaseButton variant="secondary" size="sm" @click="validationOpen = false">
          {{ t("common.close") }}
        </BaseButton>
        <BaseButton variant="primary" size="sm" @click="saveWorkflow">
          {{ t("workflow.save") }}
        </BaseButton>
      </template>
    </BaseModal>
  </div>
</template>
