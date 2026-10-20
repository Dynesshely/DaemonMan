<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import {
  ChevronDown,
  Clock,
  Cpu,
  HardDrive,
  MemoryStick,
  Power,
  RefreshCw,
  Search,
  Workflow as WorkflowIcon,
} from "lucide-vue-next";

import type { ProcessInfo, TrackedProcess } from "@/api";
import { api } from "@/api";
import PageHeader from "@/modules/components/layout/PageHeader.vue";
import BaseBadge from "@/modules/components/ui/BaseBadge.vue";
import BaseButton from "@/modules/components/ui/BaseButton.vue";
import BaseModal from "@/modules/components/ui/BaseModal.vue";
import EmptyState from "@/modules/components/ui/EmptyState.vue";
import SectionCard from "@/modules/components/ui/SectionCard.vue";
import ToggleSwitch from "@/modules/components/ui/ToggleSwitch.vue";
import {
  inputClass,
  monoClass,
  selectClass,
  tableCellClass,
  tableHeadClass,
} from "@/modules/components/ui/form";
import { useI18n } from "@/modules/locales";
import { useProcessStore } from "@/modules/stores/runtime";
import { useUiStore } from "@/modules/stores/ui";
import { useWorkflowStore } from "@/modules/stores/workflows";
import {
  formatBytes,
  formatDuration,
  formatRelative,
} from "@/modules/utils/format";

const processStore = useProcessStore();
const workflowStore = useWorkflowStore();
const ui = useUiStore();
const { t, locale } = useI18n();

type ProcessView = "all" | "track";

const view = ref<ProcessView>("all");
const search = ref("");
const managedOnly = ref(false);
const killTarget = ref<{ pid: number; name: string } | null>(null);
const killBusy = ref(false);

const workflowFilter = ref("");
const tracked = ref<TrackedProcess[]>([]);
const trackedLoading = ref(false);
const trackedUpdatedAt = ref<number | null>(null);
const autoRefresh = ref(false);

const TRACK_INTERVAL_MS = 4000;
let trackTimer: number | undefined;

const isTracking = computed(() => view.value === "track");

const keyword = computed(() => search.value.trim().toLowerCase());

const rows = computed<ProcessInfo[]>(() => {
  const words = keyword.value.split(/\s+/).filter(Boolean);
  return processStore.processes
    .filter((process) => (managedOnly.value ? process.managed : true))
    .filter((process) => {
      if (words.length === 0) return true;
      const haystack = [
        process.name,
        process.workflowName ?? "",
        process.mainWindowTitle ?? "",
        process.executable ?? "",
        String(process.pid),
      ]
        .join(" ")
        .toLowerCase();
      return words.every((word) => haystack.includes(word));
    })
    .slice()
    .sort(
      (a, b) =>
        Number(b.managed) - Number(a.managed) ||
        a.name.localeCompare(b.name, "en"),
    );
});

const managedCount = computed(
  () => processStore.processes.filter((process) => process.managed).length,
);

/** 进程数可能上千，先渲染一页，避免一次插入几万个 DOM 节点。 */
const PAGE_SIZE = 200;
const visibleLimit = ref(PAGE_SIZE);
const visibleRows = computed(() => rows.value.slice(0, visibleLimit.value));
const hiddenCount = computed(() =>
  Math.max(0, rows.value.length - visibleRows.value.length),
);

watch([keyword, managedOnly], () => {
  visibleLimit.value = PAGE_SIZE;
});

function loadMore() {
  visibleLimit.value += PAGE_SIZE;
}

const trackedRows = computed(() =>
  tracked.value.slice().sort(
    (a, b) =>
      a.workflowName.localeCompare(b.workflowName, "en") ||
      a.nodeName.localeCompare(b.nodeName, "en") ||
      a.name.localeCompare(b.name, "en"),
  ),
);

const trackedManagedCount = computed(
  () => tracked.value.filter((process) => process.managed).length,
);

const updatedAt = computed(() => {
  if (!processStore.lastUpdated) return null;
  return formatRelative(
    new Date(processStore.lastUpdated).toISOString(),
    locale.value,
  );
});

const trackedUpdatedLabel = computed(() => {
  if (!trackedUpdatedAt.value) return null;
  return formatRelative(
    new Date(trackedUpdatedAt.value).toISOString(),
    locale.value,
  );
});

const showSkeleton = computed(
  () => processStore.loading && processStore.processes.length === 0,
);

const showTrackSkeleton = computed(
  () => trackedLoading.value && tracked.value.length === 0,
);

async function refreshAll() {
  try {
    await processStore.load();
  } catch (error) {
    ui.notify("error", t("error.invoke", { error: String(error) }));
  }
}

/** 按当前工作流筛选拉取追踪结果（null = 全部工作流）。 */
async function fetchTracked(
  workflowId: string | null,
): Promise<TrackedProcess[]> {
  return api.trackWorkflowProcesses(workflowId);
}

async function loadTracked() {
  if (!isTracking.value) return;
  trackedLoading.value = true;
  try {
    tracked.value = await fetchTracked(workflowFilter.value || null);
    trackedUpdatedAt.value = Date.now();
  } catch (error) {
    ui.notify("error", t("error.invoke", { error: String(error) }));
  } finally {
    trackedLoading.value = false;
  }
}

function stopTrackTimer() {
  if (trackTimer !== undefined) {
    window.clearInterval(trackTimer);
    trackTimer = undefined;
  }
}

function syncTrackTimer() {
  stopTrackTimer();
  if (!isTracking.value || !autoRefresh.value) return;
  trackTimer = window.setInterval(() => {
    if (isTracking.value) {
      void loadTracked();
    }
  }, TRACK_INTERVAL_MS);
}

async function onRefresh() {
  if (isTracking.value) {
    await loadTracked();
    return;
  }
  await refreshAll();
}

function askKill(target: { pid: number; name: string }) {
  killTarget.value = target;
}

function closeKill() {
  if (killBusy.value) return;
  killTarget.value = null;
}

async function confirmKill() {
  const target = killTarget.value;
  if (!target) return;
  killBusy.value = true;
  try {
    await processStore.kill(target.pid);
    killTarget.value = null;
    if (isTracking.value) {
      await loadTracked();
    }
  } catch (error) {
    ui.notify("error", t("process.killFailed", { error: String(error) }));
  } finally {
    killBusy.value = false;
  }
}

watch(view, (next) => {
  if (next === "track") {
    void loadTracked();
  }
  syncTrackTimer();
});

watch(workflowFilter, () => {
  if (isTracking.value) {
    void loadTracked();
  }
});

watch(autoRefresh, syncTrackTimer);

onMounted(async () => {
  await Promise.allSettled([
    refreshAll(),
    workflowStore.workflows.length === 0
      ? workflowStore.load()
      : Promise.resolve(),
  ]);
});

onBeforeUnmount(stopTrackTimer);
</script>

<template>
  <div>
    <PageHeader
      :title="t('process.title')"
      :description="t('process.subtitle')"
    />

    <SectionCard :padded="false" dense>
      <!-- 视图切换 -->
      <div
        class="flex flex-wrap items-center gap-1 border-b border-zinc-200/80 px-3 py-2 dark:border-zinc-800/80"
      >
        <button
          type="button"
          class="inline-flex items-center gap-1.5 rounded-full border px-2.5 py-1 text-[11px] font-medium outline-none transition focus-visible:ring-2 focus-visible:ring-brand-500/50"
          :class="
            view === 'all'
              ? 'border-brand-600 bg-brand-600 text-white shadow-sm shadow-brand-600/25 dark:border-brand-500 dark:bg-brand-500'
              : 'border-zinc-200 bg-white text-zinc-600 hover:border-zinc-300 hover:bg-zinc-50 dark:border-zinc-700 dark:bg-zinc-900 dark:text-zinc-300 dark:hover:border-zinc-600 dark:hover:bg-zinc-800'
          "
          :aria-pressed="view === 'all'"
          @click="view = 'all'"
        >
          <MemoryStick class="h-3 w-3" />
          {{ t("process.tab.all") }}
        </button>
        <button
          type="button"
          class="inline-flex items-center gap-1.5 rounded-full border px-2.5 py-1 text-[11px] font-medium outline-none transition focus-visible:ring-2 focus-visible:ring-brand-500/50"
          :class="
            view === 'track'
              ? 'border-brand-600 bg-brand-600 text-white shadow-sm shadow-brand-600/25 dark:border-brand-500 dark:bg-brand-500'
              : 'border-zinc-200 bg-white text-zinc-600 hover:border-zinc-300 hover:bg-zinc-50 dark:border-zinc-700 dark:bg-zinc-900 dark:text-zinc-300 dark:hover:border-zinc-600 dark:hover:bg-zinc-800'
          "
          :aria-pressed="view === 'track'"
          @click="view = 'track'"
        >
          <WorkflowIcon class="h-3 w-3" />
          {{ t("process.tab.workflow") }}
        </button>
      </div>

      <!-- 全部进程工具栏 -->
      <div
        v-if="!isTracking"
        class="flex flex-col gap-2.5 border-b border-zinc-200/80 p-3 dark:border-zinc-800/80 lg:flex-row lg:items-center"
      >
        <div class="relative min-w-0 flex-1">
          <Search
            class="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-zinc-400"
          />
          <input
            v-model="search"
            :class="[inputClass, 'pl-9']"
            type="search"
            :placeholder="t('process.searchPlaceholder')"
          />
        </div>

        <label
          class="flex shrink-0 cursor-pointer items-center gap-2 rounded-lg border border-zinc-200 px-2.5 py-1.5 text-xs text-zinc-600 dark:border-zinc-700 dark:text-zinc-300"
        >
          <ToggleSwitch v-model="managedOnly" size="sm" />
          <span>{{ t("process.managedOnly") }}</span>
        </label>

        <div
          class="flex shrink-0 flex-wrap items-center gap-x-3 gap-y-1 text-[11px] text-zinc-500 dark:text-zinc-400"
        >
          <span
            class="inline-flex items-center gap-1 text-brand-600 dark:text-brand-400"
          >
            <MemoryStick class="h-3 w-3" />
            {{ t("process.managedBadge") }} {{ managedCount }}
          </span>
          <span>{{ t("process.count", { count: rows.length }) }}</span>
          <span v-if="updatedAt" class="inline-flex items-center gap-1">
            <Clock class="h-3 w-3" />
            {{ updatedAt }}
          </span>
          <BaseButton
            variant="ghost"
            size="sm"
            icon-only
            :title="t('process.refresh')"
            :loading="processStore.loading"
            :disabled="processStore.loading"
            @click="onRefresh"
          >
            <RefreshCw v-if="!processStore.loading" class="h-3.5 w-3.5" />
          </BaseButton>
        </div>
      </div>

      <!-- 工作流追踪工具栏 -->
      <div
        v-else
        class="flex flex-col gap-2.5 border-b border-zinc-200/80 p-3 dark:border-zinc-800/80 lg:flex-row lg:items-center"
      >
        <div class="flex min-w-0 items-center gap-2">
          <span
            class="shrink-0 text-[11px] font-semibold uppercase tracking-wider text-zinc-500 dark:text-zinc-400"
          >
            {{ t("process.workflowFilter") }}
          </span>
          <div class="relative min-w-0 flex-1 sm:w-64 sm:flex-none">
            <select v-model="workflowFilter" :class="selectClass">
              <option value="">{{ t("process.allWorkflows") }}</option>
              <option
                v-for="workflow in workflowStore.workflows"
                :key="workflow.id"
                :value="workflow.id"
              >
                {{ workflow.name }}
              </option>
            </select>
            <ChevronDown
              class="pointer-events-none absolute right-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-zinc-400"
            />
          </div>
        </div>

        <label
          class="flex shrink-0 cursor-pointer items-center gap-2 rounded-lg border border-zinc-200 px-2.5 py-1.5 text-xs text-zinc-600 dark:border-zinc-700 dark:text-zinc-300"
        >
          <ToggleSwitch v-model="autoRefresh" size="sm" />
          <span>{{ t("process.autoRefresh") }}</span>
        </label>

        <div
          class="flex shrink-0 flex-wrap items-center gap-x-3 gap-y-1 text-[11px] text-zinc-500 dark:text-zinc-400"
        >
          <span
            class="inline-flex items-center gap-1 text-brand-600 dark:text-brand-400"
          >
            <MemoryStick class="h-3 w-3" />
            {{ t("process.managedBadge") }} {{ trackedManagedCount }}
          </span>
          <span>{{ t("process.count", { count: trackedRows.length }) }}</span>
          <span v-if="trackedUpdatedLabel" class="inline-flex items-center gap-1">
            <Clock class="h-3 w-3" />
            {{ trackedUpdatedLabel }}
          </span>
          <BaseButton
            variant="ghost"
            size="sm"
            icon-only
            :title="t('process.refresh')"
            :loading="trackedLoading"
            :disabled="trackedLoading"
            @click="onRefresh"
          >
            <RefreshCw v-if="!trackedLoading" class="h-3.5 w-3.5" />
          </BaseButton>
        </div>
      </div>

      <!-- 全部进程内容 -->
      <template v-if="!isTracking">
        <div v-if="showSkeleton" class="flex flex-col gap-3 p-4">
          <div
            v-for="index in 6"
            :key="index"
            class="h-4 animate-pulse rounded bg-zinc-200 dark:bg-zinc-800"
            :class="index % 2 ? 'w-full' : 'w-11/12'"
          />
        </div>

        <div v-else-if="rows.length" class="overflow-x-auto">
          <table class="w-full min-w-[900px] border-collapse text-left">
            <thead class="bg-zinc-50/80 dark:bg-zinc-900/70">
              <tr>
                <th :class="[tableHeadClass, 'px-4 py-2.5']">
                  {{ t("common.name") }}
                </th>
                <th :class="[tableHeadClass, 'px-2 py-2.5']">
                  {{ t("process.pid") }}
                </th>
                <th :class="[tableHeadClass, 'px-2 py-2.5']">
                  {{ t("process.memory") }}
                </th>
                <th :class="[tableHeadClass, 'px-2 py-2.5']">
                  {{ t("process.cpu") }}
                </th>
                <th :class="[tableHeadClass, 'px-2 py-2.5']">
                  {{ t("process.runtime") }}
                </th>
                <th :class="[tableHeadClass, 'px-2 py-2.5']">
                  {{ t("process.windowTitle") }}
                </th>
                <th :class="[tableHeadClass, 'px-2 py-2.5']">
                  {{ t("common.operations") }}
                </th>
              </tr>
            </thead>
            <tbody class="divide-y divide-zinc-200/80 dark:divide-zinc-800/80">
              <tr
                v-for="process in visibleRows"
                :key="process.pid"
                class="transition"
                :class="
                  process.managed
                    ? 'bg-brand-50/40 dark:bg-brand-500/5'
                    : 'hover:bg-zinc-50 dark:hover:bg-zinc-800/30'
                "
              >
                <td :class="[tableCellClass, 'px-4']">
                  <div class="flex min-w-0 items-center gap-2">
                    <span
                      class="max-w-[220px] truncate font-medium text-zinc-800 dark:text-zinc-100"
                      :title="process.executable ?? process.name"
                    >
                      {{ process.name }}
                    </span>
                    <BaseBadge
                      v-if="process.managed"
                      variant="accent"
                      dot
                      :title="t('process.managed')"
                    >
                      {{ t("process.managedBadge") }}
                    </BaseBadge>
                  </div>
                  <p
                    v-if="process.workflowName"
                    class="mt-0.5 max-w-[240px] truncate text-[11px] text-brand-700 dark:text-brand-300"
                    :title="process.workflowName"
                  >
                    {{ process.workflowName }}
                  </p>
                  <p
                    v-else-if="process.executable"
                    class="mt-0.5 max-w-[240px] truncate font-mono text-[11px] text-zinc-400 dark:text-zinc-500"
                    :title="process.executable"
                  >
                    {{ process.executable }}
                  </p>
                </td>
                <td :class="[tableCellClass, 'tabular px-2 font-mono text-xs']">
                  {{ process.pid }}
                </td>
                <td :class="[tableCellClass, 'tabular whitespace-nowrap px-2']">
                  <span class="inline-flex items-center gap-1">
                    <HardDrive class="h-3 w-3 text-zinc-400" />
                    {{ formatBytes(process.memoryBytes) }}
                  </span>
                </td>
                <td :class="[tableCellClass, 'tabular whitespace-nowrap px-2']">
                  <span class="inline-flex items-center gap-1">
                    <Cpu class="h-3 w-3 text-zinc-400" />
                    {{ process.cpuUsage.toFixed(1) }}%
                  </span>
                </td>
                <td :class="[tableCellClass, 'tabular whitespace-nowrap px-2']">
                  {{ formatDuration(process.runningSecs) }}
                </td>
                <td :class="[tableCellClass, 'px-2']">
                  <span
                    v-if="process.mainWindowTitle"
                    class="block max-w-[200px] truncate text-xs text-zinc-600 dark:text-zinc-300"
                    :title="process.mainWindowTitle"
                  >
                    {{ process.mainWindowTitle }}
                  </span>
                  <span v-else :class="monoClass">—</span>
                </td>
                <td :class="[tableCellClass, 'px-2']">
                  <BaseButton
                    variant="secondary"
                    size="sm"
                    @click="askKill(process)"
                  >
                    <Power class="h-3.5 w-3.5" />
                    {{ t("process.kill") }}
                  </BaseButton>
                </td>
              </tr>
            </tbody>
          </table>

          <div
            v-if="hiddenCount > 0"
            class="flex flex-wrap items-center justify-center gap-2 border-t border-zinc-200/80 px-4 py-3 dark:border-zinc-800/80"
          >
            <span class="text-[11px] text-zinc-500 dark:text-zinc-400">
              {{ t("process.loadMore", { count: hiddenCount }) }}
            </span>
            <BaseButton variant="secondary" size="sm" @click="loadMore">
              {{ t("process.loadMoreAction") }}
            </BaseButton>
            <BaseButton
              variant="ghost"
              size="sm"
              @click="visibleLimit = rows.length"
            >
              {{ t("process.showAll") }}
            </BaseButton>
          </div>
        </div>

        <EmptyState
          v-else
          :title="t('process.empty')"
          :description="t('process.emptyHint')"
        >
          <template #action>
            <BaseButton variant="subtle" size="sm" @click="onRefresh">
              <RefreshCw class="h-3.5 w-3.5" />
              {{ t("process.refresh") }}
            </BaseButton>
          </template>
        </EmptyState>
      </template>

      <!-- 工作流追踪内容 -->
      <template v-else>
        <p
          class="border-b border-zinc-200/60 px-4 py-2 text-[11px] leading-relaxed text-zinc-500 dark:border-zinc-800/60 dark:text-zinc-400"
        >
          {{ t("process.trackHint") }}
        </p>

        <div v-if="showTrackSkeleton" class="flex flex-col gap-3 p-4">
          <div
            v-for="index in 6"
            :key="index"
            class="h-4 animate-pulse rounded bg-zinc-200 dark:bg-zinc-800"
            :class="index % 2 ? 'w-full' : 'w-11/12'"
          />
        </div>

        <div v-else-if="trackedRows.length" class="overflow-x-auto">
          <table class="w-full min-w-[980px] border-collapse text-left">
            <thead class="bg-zinc-50/80 dark:bg-zinc-900/70">
              <tr>
                <th :class="[tableHeadClass, 'px-4 py-2.5']">
                  {{ t("process.workflow") }}
                </th>
                <th :class="[tableHeadClass, 'px-2 py-2.5']">
                  {{ t("process.node") }}
                </th>
                <th :class="[tableHeadClass, 'px-2 py-2.5']">
                  {{ t("common.name") }}
                </th>
                <th :class="[tableHeadClass, 'px-2 py-2.5']">
                  {{ t("process.memory") }}
                </th>
                <th :class="[tableHeadClass, 'px-2 py-2.5']">
                  {{ t("process.cpu") }}
                </th>
                <th :class="[tableHeadClass, 'px-2 py-2.5']">
                  {{ t("process.runtime") }}
                </th>
                <th :class="[tableHeadClass, 'px-2 py-2.5']">
                  {{ t("process.source") }}
                </th>
                <th :class="[tableHeadClass, 'px-2 py-2.5']">
                  {{ t("common.operations") }}
                </th>
              </tr>
            </thead>
            <tbody class="divide-y divide-zinc-200/80 dark:divide-zinc-800/80">
              <tr
                v-for="process in trackedRows"
                :key="`${process.workflowId}:${process.nodeId}:${process.pid}`"
                class="transition"
                :class="
                  process.managed
                    ? 'bg-brand-50/40 dark:bg-brand-500/5'
                    : 'hover:bg-zinc-50 dark:hover:bg-zinc-800/30'
                "
              >
                <td :class="[tableCellClass, 'px-4']">
                  <span
                    class="block max-w-[180px] truncate text-xs font-medium text-zinc-800 dark:text-zinc-100"
                    :title="process.workflowName"
                  >
                    {{ process.workflowName }}
                  </span>
                </td>
                <td :class="[tableCellClass, 'px-2']">
                  <span
                    class="block max-w-[160px] truncate text-xs text-zinc-600 dark:text-zinc-300"
                    :title="process.nodeName"
                  >
                    {{ process.nodeName }}
                  </span>
                </td>
                <td :class="[tableCellClass, 'px-2']">
                  <div class="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-0.5">
                    <span
                      class="max-w-[200px] truncate font-medium text-zinc-800 dark:text-zinc-100"
                      :title="process.executable ?? process.name"
                    >
                      {{ process.name }}
                    </span>
                    <span
                      class="tabular shrink-0 font-mono text-[11px] text-zinc-400 dark:text-zinc-500"
                    >
                      PID {{ process.pid }}
                    </span>
                  </div>
                  <p
                    v-if="process.mainWindowTitle"
                    class="mt-0.5 max-w-[240px] truncate text-[11px] text-zinc-400 dark:text-zinc-500"
                    :title="process.mainWindowTitle"
                  >
                    {{ process.mainWindowTitle }}
                  </p>
                </td>
                <td :class="[tableCellClass, 'tabular whitespace-nowrap px-2']">
                  <span class="inline-flex items-center gap-1">
                    <HardDrive class="h-3 w-3 text-zinc-400" />
                    {{ formatBytes(process.memoryBytes) }}
                  </span>
                </td>
                <td :class="[tableCellClass, 'tabular whitespace-nowrap px-2']">
                  <span class="inline-flex items-center gap-1">
                    <Cpu class="h-3 w-3 text-zinc-400" />
                    {{ process.cpuUsage.toFixed(1) }}%
                  </span>
                </td>
                <td :class="[tableCellClass, 'tabular whitespace-nowrap px-2']">
                  {{ formatDuration(process.runningSecs) }}
                </td>
                <td :class="[tableCellClass, 'px-2']">
                  <BaseBadge
                    :variant="process.managed ? 'accent' : 'neutral'"
                    :dot="process.managed"
                  >
                    {{
                      process.managed
                        ? t("process.source.managed")
                        : t("process.source.external")
                    }}
                  </BaseBadge>
                </td>
                <td :class="[tableCellClass, 'px-2']">
                  <BaseButton
                    variant="secondary"
                    size="sm"
                    @click="askKill(process)"
                  >
                    <Power class="h-3.5 w-3.5" />
                    {{ t("process.kill") }}
                  </BaseButton>
                </td>
              </tr>
            </tbody>
          </table>
        </div>

        <EmptyState
          v-else
          :title="t('process.trackEmpty')"
          :description="t('process.trackEmptyHint')"
        >
          <template #action>
            <BaseButton variant="subtle" size="sm" @click="onRefresh">
              <RefreshCw class="h-3.5 w-3.5" />
              {{ t("process.refresh") }}
            </BaseButton>
          </template>
        </EmptyState>
      </template>
    </SectionCard>

    <BaseModal
      :open="killTarget !== null"
      :title="t('process.kill')"
      size="sm"
      :closable="!killBusy"
      @close="closeKill"
    >
      <p class="text-xs leading-relaxed text-zinc-600 dark:text-zinc-300">
        {{
          t("process.killConfirm", {
            name: killTarget?.name ?? "",
            pid: killTarget?.pid ?? "",
          })
        }}
      </p>
      <template #footer>
        <BaseButton
          variant="secondary"
          size="sm"
          :disabled="killBusy"
          @click="closeKill"
        >
          {{ t("common.cancel") }}
        </BaseButton>
        <BaseButton
          variant="danger"
          size="sm"
          :loading="killBusy"
          @click="confirmKill"
        >
          {{ t("process.kill") }}
        </BaseButton>
      </template>
    </BaseModal>
  </div>
</template>
