<script setup lang="ts">
import { computed } from "vue";
import { ChevronRight, Clock, GitBranch } from "lucide-vue-next";

import type { NodeRunState, WorkflowRun } from "@/api";
import BaseBadge from "@/modules/components/ui/BaseBadge.vue";
import { useI18n } from "@/modules/locales";
import { formatMs, formatRelative } from "@/modules/utils/format";
import {
  nodeStatusVariant,
  runStatusVariant,
  statusKey,
} from "@/modules/utils/status";

const props = defineProps<{ run: WorkflowRun }>();
const emit = defineEmits<{ open: [run: WorkflowRun] }>();

const { t, locale } = useI18n();

const nodeStates = computed<NodeRunState[]>(() =>
  Object.values(props.run.nodes ?? {}),
);

const doneCount = computed(
  () =>
    nodeStates.value.filter((node) =>
      [
        "success",
        "failed",
        "skipped",
        "skipped-existing",
        "cancelled",
      ].includes(node.status),
    ).length,
);

const totalCount = computed(() => nodeStates.value.length);

const duration = computed(() => {
  const started = new Date(props.run.startedAt).getTime();
  if (Number.isNaN(started)) return "—";
  const finished = props.run.finishedAt
    ? new Date(props.run.finishedAt).getTime()
    : Date.now();
  if (Number.isNaN(finished)) return "—";
  return formatMs(Math.max(0, finished - started));
});

const relative = computed(() =>
  formatRelative(props.run.startedAt, locale.value),
);

const dotClasses: Record<string, string> = {
  running: "bg-brand-500 animate-pulse",
  success: "bg-emerald-500",
  danger: "bg-red-500",
  warn: "bg-amber-500",
  info: "bg-sky-500",
  accent: "bg-brand-500",
  skipped: "bg-teal-500 dark:bg-teal-400",
  neutral: "bg-zinc-400 dark:bg-zinc-600",
};

const messageClass = computed(() => {
  switch (props.run.status) {
    case "failed":
      return "text-red-600 dark:text-red-400";
    case "cancelled":
      return "text-amber-600 dark:text-amber-400";
    case "success":
      return "text-emerald-600 dark:text-emerald-400";
    default:
      return "text-zinc-500 dark:text-zinc-400";
  }
});

const visibleDots = computed(() => nodeStates.value.slice(0, 28));
const hiddenDots = computed(() =>
  Math.max(0, nodeStates.value.length - visibleDots.value.length),
);
</script>

<template>
  <button
    type="button"
    class="group flex w-full items-start gap-3 px-4 py-3 text-left transition hover:bg-zinc-50 dark:hover:bg-zinc-800/40"
    @click="emit('open', run)"
  >
    <div class="min-w-0 flex-1">
      <div class="flex flex-wrap items-center gap-x-2 gap-y-1">
        <span
          class="truncate text-sm font-medium text-zinc-800 dark:text-zinc-100"
        >
          {{ run.workflowName }}
        </span>
        <BaseBadge
          :variant="runStatusVariant(run.status)"
          :dot="run.status === 'running'"
          :pulse="run.status === 'running'"
        >
          {{ t(statusKey(run.status)) }}
        </BaseBadge>
        <BaseBadge variant="neutral">
          {{ t(`trigger.${run.trigger}`) }}
        </BaseBadge>
      </div>

      <div class="mt-1.5 flex flex-wrap items-center gap-x-3 gap-y-1">
        <span
          class="inline-flex items-center gap-1 text-[11px] text-zinc-500 dark:text-zinc-400"
        >
          <GitBranch class="h-3 w-3 shrink-0" />
          <span class="tabular">
            {{ doneCount }}/{{ totalCount }}
          </span>
        </span>
        <span
          class="inline-flex items-center gap-1 text-[11px] text-zinc-500 dark:text-zinc-400"
        >
          <Clock class="h-3 w-3 shrink-0" />
          <span class="tabular">{{ duration }}</span>
        </span>
        <span class="text-[11px] text-zinc-400 dark:text-zinc-500">
          {{ relative }}
        </span>
      </div>

      <div v-if="visibleDots.length" class="mt-2 flex flex-wrap items-center gap-1">
        <span
          v-for="node in visibleDots"
          :key="node.nodeId"
          class="h-1.5 w-4 rounded-full"
          :class="dotClasses[nodeStatusVariant(node.status)] ?? dotClasses.neutral"
          :title="`${node.nodeName} · ${t(statusKey(node.status))}`"
        />
        <span
          v-if="hiddenDots > 0"
          class="text-[10px] text-zinc-400 dark:text-zinc-500"
        >
          +{{ hiddenDots }}
        </span>
      </div>

      <p
        v-if="run.message"
        class="mt-1.5 truncate text-[11px]"
        :class="messageClass"
        :title="run.message"
      >
        {{ run.message }}
      </p>
    </div>

    <ChevronRight
      class="mt-1 h-4 w-4 shrink-0 text-zinc-300 transition group-hover:translate-x-0.5 group-hover:text-zinc-500 dark:text-zinc-600 dark:group-hover:text-zinc-400"
    />
  </button>
</template>
