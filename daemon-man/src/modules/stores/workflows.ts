import { defineStore } from "pinia";
import { computed, nextTick, ref, watch } from "vue";

import { api } from "@/api";
import type {
  NodeRunState,
  Workflow,
  WorkflowRun,
  WorkflowValidation,
} from "@/api";

function clone<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T;
}

function uuid(): string {
  if (typeof crypto !== "undefined" && "randomUUID" in crypto) {
    return crypto.randomUUID();
  }
  return `${Date.now().toString(16)}-${Math.random().toString(16).slice(2)}`;
}

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
    };
    suppressDirty = true;
    draft.value = workflow;
    selectedId.value = null;
    draftDirty.value = true;
    validation.value = null;
    void nextTick(() => {
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
    }
  }

  async function remove(id: string) {
    await api.deleteWorkflow(id);
    workflows.value = workflows.value.filter((item) => item.id !== id);
    if (selectedId.value === id) {
      selectedId.value = null;
      draft.value = null;
      draftDirty.value = false;
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
  };
});
