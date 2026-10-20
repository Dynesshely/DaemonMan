import { defineStore } from "pinia";
import { computed, nextTick, ref, watch } from "vue";

import { api } from "@/api";
import type {
  NodeRunState,
  Workflow,
  WorkflowRun,
  WorkflowValidation,
} from "@/api";

import { useAppStore } from "./app";

function clone<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T;
}

function uuid(): string {
  if (typeof crypto !== "undefined" && "randomUUID" in crypto) {
    return crypto.randomUUID();
  }
  return `${Date.now().toString(16)}-${Math.random().toString(16).slice(2)}`;
}

/** 连续编辑（拖动 / 连续输入）合并为一条历史记录的防抖间隔。 */
const HISTORY_DEBOUNCE = 400;

export const useWorkflowStore = defineStore("workflows", () => {
  const workflows = ref<Workflow[]>([]);
  const runs = ref<WorkflowRun[]>([]);
  const loading = ref(false);
  const saving = ref(false);

  /** 编辑器中的草稿（未保存的修改只存在于这里） */
  const draft = ref<Workflow | null>(null);
  const draftDirty = ref(false);
  const selectedId = ref<string | null>(null);
  const validation = ref<WorkflowValidation | null>(null);

  let suppressDirty = false;

  watch(
    draft,
    () => {
      if (!suppressDirty && draft.value) {
        draftDirty.value = true;
      }
    },
    { deep: true },
  );

  // ---------------------------------------------------------------------------
  // 撤销 / 重做：快照栈（不含运行状态），上限读设置里的 undoLimit
  // ---------------------------------------------------------------------------
  const past = ref<Workflow[]>([]);
  const future = ref<Workflow[]>([]);
  const hasPendingHistory = ref(false);

  const canUndo = computed(
    () => past.value.length > 0 || hasPendingHistory.value,
  );
  const canRedo = computed(() => future.value.length > 0);

  /** 最近一次已提交快照（撤销时回退到它之前的状态）。 */
  let committed: Workflow | null = null;
  /** 已保存版本的 JSON，用于撤销后重新计算 draftDirty。 */
  let baselineJson: string | null = null;
  let suppressHistory = false;
  let historyTimer: ReturnType<typeof setTimeout> | undefined;

  function snapshot(): Workflow | null {
    return draft.value ? clone(draft.value) : null;
  }

  function resolveUndoLimit(): number {
    let limit = 100;
    try {
      limit = useAppStore().settings?.undoLimit ?? 100;
    } catch {
      limit = 100;
    }
    if (!Number.isFinite(limit)) limit = 100;
    return Math.min(10000, Math.max(1, Math.round(limit)));
  }

  function clearHistoryTimer() {
    if (historyTimer !== undefined) {
      clearTimeout(historyTimer);
      historyTimer = undefined;
    }
  }

  /**
   * 清空历史并把当前 draft 作为新起点。
   * saveBaseline=true 时同时记为「已保存版本」（载入/保存后调用）。
   */
  function resetHistory(saveBaseline: boolean) {
    clearHistoryTimer();
    past.value = [];
    future.value = [];
    hasPendingHistory.value = false;
    committed = snapshot();
    baselineJson =
      saveBaseline && committed ? JSON.stringify(committed) : null;
  }

  /** 立即提交一次快照（防抖期间也能撤销）。 */
  function commitHistory() {
    clearHistoryTimer();
    hasPendingHistory.value = false;
    if (suppressHistory) return;
    const current = snapshot();
    if (!current) return;
    const json = JSON.stringify(current);
    if (committed && JSON.stringify(committed) === json) return;
    if (committed) {
      const next = [...past.value, committed];
      const limit = resolveUndoLimit();
      past.value =
        next.length > limit ? next.slice(next.length - limit) : next;
    }
    future.value = [];
    committed = current;
  }

  function scheduleHistory() {
    if (suppressHistory) return;
    hasPendingHistory.value = true;
    clearHistoryTimer();
    historyTimer = setTimeout(() => commitHistory(), HISTORY_DEBOUNCE);
  }

  watch(
    draft,
    () => {
      if (!suppressHistory && draft.value) scheduleHistory();
    },
    { deep: true },
  );

  function applySnapshot(next: Workflow) {
    clearHistoryTimer();
    hasPendingHistory.value = false;
    suppressHistory = true;
    suppressDirty = true;
    draft.value = clone(next);
    committed = snapshot();
    draftDirty.value = baselineJson
      ? JSON.stringify(draft.value) !== baselineJson
      : true;
    validation.value = null;
    void nextTick(() => {
      suppressDirty = false;
      suppressHistory = false;
    });
  }

  function undo(): boolean {
    if (!draft.value) return false;
    commitHistory();
    const previous = past.value[past.value.length - 1];
    if (!previous) return false;
    past.value = past.value.slice(0, -1);
    future.value = [...future.value, clone(draft.value)];
    applySnapshot(previous);
    return true;
  }

  function redo(): boolean {
    if (!draft.value) return false;
    commitHistory();
    const next = future.value[future.value.length - 1];
    if (!next) return false;
    future.value = future.value.slice(0, -1);
    past.value = [...past.value, clone(draft.value)];
    applySnapshot(next);
    return true;
  }

  const runningCount = computed(
    () => runs.value.filter((run) => run.status === "running").length,
  );

  const draftRun = computed<WorkflowRun | null>(() => {
    if (!draft.value) return null;
    return (
      runs.value.find((run) => run.workflowId === draft.value?.id) ?? null
    );
  });

  function nodeRun(nodeId: string): NodeRunState | null {
    return draftRun.value?.nodes[nodeId] ?? null;
  }

  function latestRunFor(workflowId: string): WorkflowRun | null {
    return runs.value.find((run) => run.workflowId === workflowId) ?? null;
  }

  async function load() {
    loading.value = true;
    try {
      workflows.value = await api.listWorkflows();
    } finally {
      loading.value = false;
    }
  }

  async function loadRuns() {
    runs.value = await api.getRecentRuns();
  }

  function createDraft(name: string) {
    const now = new Date().toISOString();
    const workflow: Workflow = {
      id: uuid(),
      name,
      description: "",
      enabled: false,
      createdAt: now,
      updatedAt: now,
      nodes: [],
      edges: [],
      skipExistingProcesses: true,
    };
    suppressDirty = true;
    draft.value = workflow;
    selectedId.value = null;
    draftDirty.value = true;
    validation.value = null;
    void nextTick(() => {
      resetHistory(false);
      suppressDirty = false;
    });
    return workflow;
  }

  async function loadDraft(id: string) {
    const found = workflows.value.find((workflow) => workflow.id === id);
    if (!found) return;
    suppressDirty = true;
    draft.value = clone(found);
    selectedId.value = id;
    draftDirty.value = false;
    validation.value = null;
    await nextTick();
    resetHistory(true);
    suppressDirty = false;
  }

  async function saveDraft(): Promise<Workflow | null> {
    if (!draft.value) return null;
    saving.value = true;
    try {
      const saved = await api.saveWorkflow(draft.value);
      const index = workflows.value.findIndex((item) => item.id === saved.id);
      if (index >= 0) {
        workflows.value[index] = saved;
      } else {
        workflows.value.push(saved);
      }
      suppressDirty = true;
      draft.value = clone(saved);
      selectedId.value = saved.id;
      draftDirty.value = false;
      await nextTick();
      resetHistory(true);
      suppressDirty = false;
      return saved;
    } finally {
      saving.value = false;
    }
  }

  function discardDraft() {
    if (!draft.value) return;
    if (selectedId.value) {
      void loadDraft(selectedId.value);
    } else {
      draft.value = null;
      draftDirty.value = false;
      validation.value = null;
      resetHistory(false);
    }
  }

  async function remove(id: string) {
    await api.deleteWorkflow(id);
    workflows.value = workflows.value.filter((item) => item.id !== id);
    if (selectedId.value === id) {
      selectedId.value = null;
      draft.value = null;
      draftDirty.value = false;
      resetHistory(false);
    }
  }

  async function duplicate(id: string) {
    const copy = await api.duplicateWorkflow(id);
    workflows.value.push(copy);
    return copy;
  }

  async function validateDraft(): Promise<WorkflowValidation | null> {
    if (!draft.value) return null;
    validation.value = await api.validateWorkflow(draft.value);
    return validation.value;
  }

  async function run(workflowId: string) {
    const run = await api.runWorkflow(workflowId);
    applyRun(run);
    return run;
  }

  async function cancel(runId: string) {
    await api.cancelRun(runId);
  }

  async function runAll() {
    const created = await api.runAllEnabled();
    created.forEach(applyRun);
    return created;
  }

  function applyRun(run: WorkflowRun) {
    const index = runs.value.findIndex((item) => item.runId === run.runId);
    if (index >= 0) {
      runs.value[index] = run;
    } else {
      runs.value.unshift(run);
      if (runs.value.length > 50) {
        runs.value.pop();
      }
    }
    // 保持最新在前
    runs.value = [...runs.value].sort((a, b) =>
      a.startedAt < b.startedAt ? 1 : -1,
    );
  }

  return {
    workflows,
    runs,
    loading,
    saving,
    draft,
    draftDirty,
    selectedId,
    validation,
    runningCount,
    draftRun,
    nodeRun,
    latestRunFor,
    canUndo,
    canRedo,
    load,
    loadRuns,
    createDraft,
    loadDraft,
    saveDraft,
    discardDraft,
    remove,
    duplicate,
    validateDraft,
    run,
    cancel,
    runAll,
    applyRun,
    undo,
    redo,
  };
});
