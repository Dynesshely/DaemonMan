<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
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

import type { WorkflowValidation } from "@/api";
import PageHeader from "@/modules/components/layout/PageHeader.vue";
import BaseBadge from "@/modules/components/ui/BaseBadge.vue";
import BaseButton from "@/modules/components/ui/BaseButton.vue";
import BaseModal from "@/modules/components/ui/BaseModal.vue";
import BaseSpinner from "@/modules/components/ui/BaseSpinner.vue";
import EmptyState from "@/modules/components/ui/EmptyState.vue";
import { useI18n } from "@/modules/locales";
import { useUiStore } from "@/modules/stores/ui";
import { useWorkflowStore } from "@/modules/stores/workflows";

import WorkflowCanvas from "@/modules/components/workflow/WorkflowCanvas.vue";
import WorkflowInspector from "@/modules/components/workflow/WorkflowInspector.vue";
import WorkflowListPanel from "@/modules/components/workflow/WorkflowListPanel.vue";
import WorkflowRunHistory from "@/modules/components/workflow/WorkflowRunHistory.vue";
import WorkflowTextView from "@/modules/components/workflow/WorkflowTextView.vue";
import WorkflowToolbar from "@/modules/components/workflow/WorkflowToolbar.vue";
import { errorText } from "@/modules/components/workflow/shared";
import type {
  WorkflowSelection,
  WorkflowViewMode,
} from "@/modules/components/workflow/types";

const store = useWorkflowStore();
const ui = useUiStore();
const { t } = useI18n();

const canvasRef = ref<InstanceType<typeof WorkflowCanvas> | null>(null);

const selectedNodeId = ref<string | null>(null);
const selectedEdgeId = ref<string | null>(null);

/** 节点画布 / 高密度文本视图。 */
const viewMode = ref<WorkflowViewMode>("nodes");

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
function onKeydown(event: KeyboardEvent) {
  if (!(event.ctrlKey || event.metaKey) || event.key.toLowerCase() !== "s") return;
  event.preventDefault();
  if (!store.draft || event.repeat) return;
  void saveWorkflow();
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
            ref="canvasRef"
            class="h-full flex-1"
            :selected-node-id="selectedNodeId"
            @selection-change="onSelectionChange"
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
      />
    </div>

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
