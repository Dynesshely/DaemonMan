<script setup lang="ts">
import { computed, ref } from "vue";
import { ChevronDown, History } from "lucide-vue-next";

import type { WorkflowRun } from "@/api";
import BaseBadge from "@/modules/components/ui/BaseBadge.vue";
import EmptyState from "@/modules/components/ui/EmptyState.vue";
import { useI18n } from "@/modules/locales";
import { formatDuration, formatTimeOnly } from "@/modules/utils/format";
import { runStatusVariant } from "@/modules/utils/status";

import { nodeStatusDot } from "./meta";
import { panelClass, panelHeaderClass } from "./shared";

const props = defineProps<{ runs: WorkflowRun[] }>();
const { t } = useI18n();

const expandedRunId = ref<string | null>(null);

function toggle(runId: string) {
  expandedRunId.value = expandedRunId.value === runId ? null : runId;
}

function durationLabel(run: WorkflowRun): string {
  if (!run.finishedAt) return "—";
  const started = new Date(run.startedAt).getTime();
  const finished = new Date(run.finishedAt).getTime();
  if (Number.isNaN(started) || Number.isNaN(finished)) return "—";
  return formatDuration(Math.max(0, (finished - started) / 1000));
}

function progressLabel(run: WorkflowRun): string {
  const nodes = Object.values(run.nodes);
  const done = nodes.filter(
    (node) =>
      node.status === "success" ||
      node.status === "failed" ||
      node.status === "skipped" ||
      node.status === "skipped-existing" ||
      node.status === "cancelled",
  ).length;
  return `${done}/${nodes.length}`;
}

const hasRuns = computed(() => props.runs.length > 0);
</script>

<template>
  <section :class="panelClass">
    <header :class="panelHeaderClass">
      <div class="flex items-center gap-1.5">
        <History class="h-3.5 w-3.5 text-zinc-400 dark:text-zinc-500" />
        <p class="text-[11px] font-semibold uppercase tracking-wider text-zinc-500 dark:text-zinc-400">
          {{ t("workflow.runHistory") }}
        </p>
      </div>
      <BaseBadge v-if="hasRuns" variant="neutral">{{ props.runs.length }}</BaseBadge>
    </header>

    <div class="min-h-0 flex-1 overflow-y-auto p-2">
      <EmptyState
        v-if="!hasRuns"
        :title="t('workflow.noRuns')"
        :description="t('overview.noRunsHint')"
        compact
      >
        <template #icon>
          <History class="h-5 w-5" />
        </template>
      </EmptyState>

      <ul v-else class="space-y-1">
        <li v-for="run in props.runs" :key="run.runId">
          <button
            type="button"
            class="w-full rounded-lg border border-transparent px-2 py-1.5 text-left transition hover:border-zinc-200 hover:bg-zinc-50 dark:hover:border-zinc-800 dark:hover:bg-zinc-800/50"
            @click="toggle(run.runId)"
          >
            <div class="flex items-center gap-2">
              <BaseBadge
                :variant="runStatusVariant(run.status)"
                dot
                :pulse="run.status === 'running'"
              >
                {{ t(`status.${run.status}`) }}
              </BaseBadge>
              <span class="shrink-0 text-[11px] text-zinc-500 dark:text-zinc-400">
                {{ t(`trigger.${run.trigger}`) }}
              </span>
              <span class="tabular shrink-0 text-[11px] text-zinc-400 dark:text-zinc-500">
                {{ formatTimeOnly(run.startedAt) }}
              </span>
              <span class="ml-auto tabular shrink-0 text-[11px] text-zinc-400 dark:text-zinc-500">
                {{ durationLabel(run) }}
              </span>
              <span class="tabular shrink-0 text-[11px] text-zinc-400 dark:text-zinc-500">
                {{ progressLabel(run) }}
              </span>
              <ChevronDown
                class="h-3 w-3 shrink-0 text-zinc-400 transition-transform"
                :class="expandedRunId === run.runId ? 'rotate-180' : ''"
              />
            </div>

            <div
              v-if="expandedRunId === run.runId"
              class="mt-1.5 space-y-1 border-t border-zinc-100 pt-1.5 dark:border-zinc-800"
            >
              <div
                v-for="state in Object.values(run.nodes)"
                :key="state.nodeId"
                class="flex items-center gap-2 text-[11px]"
              >
                <span
                  class="h-1.5 w-1.5 shrink-0 rounded-full"
                  :class="nodeStatusDot[state.status]"
                />
                <span class="min-w-0 flex-1 truncate text-zinc-600 dark:text-zinc-300">
                  {{ state.nodeName }}
                </span>
                <span class="shrink-0 text-zinc-400 dark:text-zinc-500">
                  {{ t(`status.${state.status}`) }}
                </span>
              </div>
              <p
                v-if="run.message"
                class="break-words text-[11px] leading-relaxed text-red-500 dark:text-red-400"
              >
                {{ run.message }}
              </p>
            </div>
          </button>
        </li>
      </ul>
    </div>
  </section>
</template>
