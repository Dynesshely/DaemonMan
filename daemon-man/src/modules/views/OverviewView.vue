<script setup lang="ts">
import { computed, onMounted } from "vue";
import { useRouter } from "vue-router";
import {
  ArrowRight,
  FolderCog,
  FolderOpen,
  ListChecks,
  Play,
  Plus,
  RefreshCw,
  Rocket,
  ScrollText,
  Zap,
} from "lucide-vue-next";

import type { Workflow } from "@/api";
import { api } from "@/api";
import PageHeader from "@/modules/components/layout/PageHeader.vue";
import BaseBadge from "@/modules/components/ui/BaseBadge.vue";
import BaseButton from "@/modules/components/ui/BaseButton.vue";
import EmptyState from "@/modules/components/ui/EmptyState.vue";
import SectionCard from "@/modules/components/ui/SectionCard.vue";
import RecentRunItem from "@/modules/components/dashboard/RecentRunItem.vue";
import StatCard from "@/modules/components/dashboard/StatCard.vue";
import SystemInfoPanel from "@/modules/components/dashboard/SystemInfoPanel.vue";
import { useI18n } from "@/modules/locales";
import { useAppStore } from "@/modules/stores/app";
import { useAutostartStore } from "@/modules/stores/autostart";
import { useLogStore } from "@/modules/stores/logs";
import { useUiStore } from "@/modules/stores/ui";
import { useWorkflowStore } from "@/modules/stores/workflows";
import { formatRelative } from "@/modules/utils/format";
import { runStatusVariant, statusKey } from "@/modules/utils/status";

const router = useRouter();
const appStore = useAppStore();
const autostartStore = useAutostartStore();
const workflowStore = useWorkflowStore();
const logStore = useLogStore();
const ui = useUiStore();
const { t, locale } = useI18n();

const recentRuns = computed(() => workflowStore.runs.slice(0, 6));
const pipeline = computed(() =>
  workflowStore.workflows.filter((workflow) => workflow.enabled),
);

const pipelineRows = computed(() =>
  pipeline.value.map((workflow) => ({
    workflow,
    run: workflowStore.latestRunFor(workflow.id),
  })),
);

const autostartTotal = computed(() => autostartStore.items.length);
const autostartHint = computed(() =>
  t("overview.autostartValue", {
    enabled: autostartStore.enabledCount,
    total: autostartTotal.value,
  }),
);

const pipelineHint = computed(() => t("overview.pipelineHint"));

const lastScan = computed(() => {
  if (!autostartStore.scannedAt) return null;
  return formatRelative(
    new Date(autostartStore.scannedAt).toISOString(),
    locale.value,
  );
});

async function runAll() {
  try {
    const runs = await workflowStore.runAll();
    if (runs.length === 0) {
      ui.notify("info", t("overview.noPipeline"));
    } else {
      ui.notify("success", `${t("overview.runAll")} · ${runs.length}`);
    }
  } catch (error) {
    ui.notify("error", `${t("workflow.runFailed")}：${String(error)}`);
  }
}

async function runWorkflow(workflow: Workflow) {
  try {
    await workflowStore.run(workflow.id);
    ui.notify("success", t("workflow.runStarted", { name: workflow.name }));
  } catch (error) {
    ui.notify("error", `${t("workflow.runFailed")}：${String(error)}`);
  }
}

async function scanAutostart() {
  await autostartStore.scan(true);
  if (autostartStore.error) {
    ui.notify("error", String(autostartStore.error));
  } else {
    ui.notify("success", t("startup.totalCount", { count: autostartTotal.value }));
  }
}

async function openConfigDir() {
  try {
    await api.openConfigDir();
  } catch (error) {
    ui.notify("error", String(error));
  }
}

async function openLogDir() {
  try {
    await api.openLogDir();
  } catch (error) {
    ui.notify("error", String(error));
  }
}

function openWorkflows() {
  void router.push("/workflows");
}

function openStartup() {
  void router.push("/startup");
}

onMounted(async () => {
  await Promise.allSettled([
    appStore.refreshSystemInfo(),
    workflowStore.loadRuns(),
    workflowStore.load(),
    autostartStore.scan(),
    logStore.loaded ? Promise.resolve() : logStore.load(),
  ]);
});
</script>

<template>
  <div>
    <PageHeader
      :title="t('overview.title')"
      :description="t('overview.subtitle')"
    />

    <div class="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
      <StatCard
        :label="t('overview.workflowCard')"
        :value="workflowStore.workflows.length"
        :hint="t('overview.workflowValue', { count: workflowStore.workflows.length })"
        :icon="ListChecks"
        tone="brand"
      />
      <StatCard
        :label="t('overview.runningCard')"
        :value="workflowStore.runningCount"
        :hint="t('overview.runningValue', { count: workflowStore.runningCount })"
        :icon="Zap"
        tone="emerald"
      />
      <StatCard
        :label="t('overview.autostartCard')"
        :value="`${autostartStore.enabledCount} / ${autostartTotal}`"
        :hint="autostartHint"
        :icon="Rocket"
        tone="amber"
      />
      <StatCard
        :label="t('overview.logCard')"
        :value="logStore.entries.length"
        :hint="t('overview.logValue', { count: logStore.entries.length })"
        :icon="ScrollText"
        tone="sky"
      />
    </div>

    <div class="mt-4 grid gap-4 xl:grid-cols-3">
      <div class="flex min-w-0 flex-col gap-4 xl:col-span-2">
        <SectionCard
          :title="t('overview.recentRuns')"
          :padded="false"
          dense
        >
          <template #actions>
            <BaseButton variant="ghost" size="sm" @click="openWorkflows">
              {{ t("nav.workflows") }}
              <ArrowRight class="h-3.5 w-3.5" />
            </BaseButton>
          </template>

          <div
            v-if="recentRuns.length"
            class="divide-y divide-zinc-200/80 dark:divide-zinc-800/80"
          >
            <RecentRunItem
              v-for="run in recentRuns"
              :key="run.runId"
              :run="run"
              @open="openWorkflows"
            />
          </div>
          <EmptyState
            v-else
            compact
            :title="t('overview.noRuns')"
            :description="t('overview.noRunsHint')"
          >
            <template #action>
              <BaseButton variant="subtle" size="sm" @click="openWorkflows">
                <Play class="h-3.5 w-3.5" />
                {{ t("workflow.run") }}
              </BaseButton>
            </template>
          </EmptyState>
        </SectionCard>

        <SectionCard
          :title="t('overview.startupPipeline')"
          :description="pipelineHint"
          :padded="false"
          dense
        >
          <template #actions>
            <BaseBadge variant="accent">
              {{ pipeline.length }}
            </BaseBadge>
          </template>

          <ul
            v-if="pipelineRows.length"
            class="divide-y divide-zinc-200/80 dark:divide-zinc-800/80"
          >
            <li
              v-for="row in pipelineRows"
              :key="row.workflow.id"
              class="flex items-center gap-3 px-4 py-2.5"
            >
              <span
                class="flex h-7 w-7 shrink-0 items-center justify-center rounded-lg bg-brand-50 text-brand-600 dark:bg-brand-500/15 dark:text-brand-300"
              >
                <Rocket class="h-3.5 w-3.5" />
              </span>
              <div class="min-w-0 flex-1">
                <p
                  class="truncate text-sm font-medium text-zinc-800 dark:text-zinc-100"
                >
                  {{ row.workflow.name }}
                </p>
                <p
                  class="mt-0.5 flex flex-wrap items-center gap-x-2 gap-y-0.5 text-[11px] text-zinc-500 dark:text-zinc-400"
                >
                  <span>
                    {{
                      t("workflow.nodeCount", {
                        count: row.workflow.nodes.length,
                      })
                    }}
                  </span>
                  <template v-if="row.run">
                    <BaseBadge :variant="runStatusVariant(row.run.status)">
                      {{ t(statusKey(row.run.status)) }}
                    </BaseBadge>
                    <span :title="t('overview.lastRun')">
                      {{ formatRelative(row.run.startedAt, locale) }}
                    </span>
                  </template>
                  <span v-else class="text-zinc-400 dark:text-zinc-500">
                    {{ t("common.never") }}
                  </span>
                </p>
              </div>
              <BaseButton
                variant="subtle"
                size="sm"
                :title="t('overview.runWorkflow')"
                @click="runWorkflow(row.workflow)"
              >
                <Play class="h-3.5 w-3.5" />
                {{ t("overview.runWorkflow") }}
              </BaseButton>
            </li>
          </ul>

          <EmptyState
            v-else
            compact
            :title="t('overview.noPipeline')"
            :description="t('overview.enableInEditor')"
          >
            <template #action>
              <BaseButton variant="subtle" size="sm" @click="openWorkflows">
                <ListChecks class="h-3.5 w-3.5" />
                {{ t("nav.workflows") }}
              </BaseButton>
            </template>
          </EmptyState>
        </SectionCard>
      </div>

      <div class="flex min-w-0 flex-col gap-4">
        <SectionCard :title="t('overview.quickActions')" dense>
          <div class="flex flex-col gap-1.5">
            <button
              type="button"
              class="group flex items-center gap-3 rounded-lg px-2.5 py-2 text-left transition hover:bg-zinc-100 dark:hover:bg-zinc-800/60"
              @click="runAll"
            >
              <span
                class="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-emerald-50 text-emerald-600 dark:bg-emerald-500/15 dark:text-emerald-300"
              >
                <Play class="h-4 w-4" />
              </span>
              <span class="min-w-0 flex-1">
                <span
                  class="block truncate text-xs font-medium text-zinc-800 dark:text-zinc-100"
                >
                  {{ t("overview.runAll") }}
                </span>
              </span>
              <ArrowRight
                class="h-3.5 w-3.5 shrink-0 text-zinc-300 transition group-hover:translate-x-0.5 group-hover:text-zinc-500 dark:text-zinc-600"
              />
            </button>

            <button
              type="button"
              class="group flex items-center gap-3 rounded-lg px-2.5 py-2 text-left transition hover:bg-zinc-100 dark:hover:bg-zinc-800/60 disabled:opacity-60"
              :disabled="autostartStore.scanning"
              @click="scanAutostart"
            >
              <span
                class="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-amber-50 text-amber-600 dark:bg-amber-500/15 dark:text-amber-300"
              >
                <RefreshCw
                  class="h-4 w-4"
                  :class="autostartStore.scanning ? 'animate-spin' : ''"
                />
              </span>
              <span class="min-w-0 flex-1">
                <span
                  class="block truncate text-xs font-medium text-zinc-800 dark:text-zinc-100"
                >
                  {{ t("overview.scanStartup") }}
                </span>
                <span
                  v-if="lastScan"
                  class="block truncate text-[11px] text-zinc-500 dark:text-zinc-400"
                >
                  {{ t("startup.lastScan", { time: lastScan }) }}
                </span>
              </span>
              <ArrowRight
                class="h-3.5 w-3.5 shrink-0 text-zinc-300 transition group-hover:translate-x-0.5 group-hover:text-zinc-500 dark:text-zinc-600"
              />
            </button>

            <button
              type="button"
              class="group flex items-center gap-3 rounded-lg px-2.5 py-2 text-left transition hover:bg-zinc-100 dark:hover:bg-zinc-800/60"
              @click="openWorkflows"
            >
              <span
                class="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-brand-50 text-brand-600 dark:bg-brand-500/15 dark:text-brand-300"
              >
                <Plus class="h-4 w-4" />
              </span>
              <span class="min-w-0 flex-1">
                <span
                  class="block truncate text-xs font-medium text-zinc-800 dark:text-zinc-100"
                >
                  {{ t("overview.newWorkflow") }}
                </span>
              </span>
              <ArrowRight
                class="h-3.5 w-3.5 shrink-0 text-zinc-300 transition group-hover:translate-x-0.5 group-hover:text-zinc-500 dark:text-zinc-600"
              />
            </button>
          </div>
        </SectionCard>

        <SectionCard :title="t('overview.systemInfo')" dense>
          <SystemInfoPanel
            :info="appStore.systemInfo"
            @open-config="openConfigDir"
            @open-log="openLogDir"
          />
        </SectionCard>

        <SectionCard :title="t('overview.autostartSummary')" dense>
          <div class="grid grid-cols-3 gap-2">
            <div
              class="rounded-lg border border-zinc-200/80 px-2 py-2 text-center dark:border-zinc-800/80"
            >
              <p class="tabular text-lg font-semibold text-emerald-600 dark:text-emerald-400">
                {{ autostartStore.enabledCount }}
              </p>
              <p class="mt-0.5 text-[11px] text-zinc-500 dark:text-zinc-400">
                {{ t("overview.enabled") }}
              </p>
            </div>
            <div
              class="rounded-lg border border-zinc-200/80 px-2 py-2 text-center dark:border-zinc-800/80"
            >
              <p class="tabular text-lg font-semibold text-zinc-500 dark:text-zinc-400">
                {{ autostartStore.disabledCount }}
              </p>
              <p class="mt-0.5 text-[11px] text-zinc-500 dark:text-zinc-400">
                {{ t("overview.disabled") }}
              </p>
            </div>
            <div
              class="rounded-lg border border-zinc-200/80 px-2 py-2 text-center dark:border-zinc-800/80"
            >
              <p class="tabular text-lg font-semibold text-brand-600 dark:text-brand-400">
                {{ autostartStore.takenOverCount }}
              </p>
              <p class="mt-0.5 text-[11px] text-zinc-500 dark:text-zinc-400">
                {{ t("overview.takenOver") }}
              </p>
            </div>
          </div>

          <div
            v-if="autostartStore.scanning && !autostartTotal"
            class="mt-2 flex items-center gap-2 text-[11px] text-zinc-500 dark:text-zinc-400"
          >
            <RefreshCw class="h-3 w-3 animate-spin" />
            {{ t("startup.scanning") }}
          </div>

          <BaseButton
            variant="secondary"
            size="sm"
            block
            class="mt-3"
            @click="openStartup"
          >
            <FolderCog class="h-3.5 w-3.5" />
            {{ t("overview.goStartup") }}
          </BaseButton>
        </SectionCard>

        <div
          class="flex items-start gap-2 rounded-xl border border-zinc-200 bg-white/60 px-3 py-2.5 text-[11px] leading-relaxed text-zinc-500 dark:border-zinc-800 dark:bg-zinc-900/40 dark:text-zinc-400"
        >
          <FolderOpen class="mt-0.5 h-3.5 w-3.5 shrink-0" />
          <span>{{ t("settings.hint") }}</span>
        </div>
      </div>
    </div>
  </div>
</template>
