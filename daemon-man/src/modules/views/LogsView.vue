<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch } from "vue";
import { save } from "@tauri-apps/plugin-dialog";
import {
  ChevronDown,
  ChevronRight,
  Download,
  RefreshCw,
  Search,
  Trash2,
} from "lucide-vue-next";

import type { LogEntry, LogLevel } from "@/api";
import PageHeader from "@/modules/components/layout/PageHeader.vue";
import BaseBadge from "@/modules/components/ui/BaseBadge.vue";
import BaseButton from "@/modules/components/ui/BaseButton.vue";
import BaseModal from "@/modules/components/ui/BaseModal.vue";
import EmptyState from "@/modules/components/ui/EmptyState.vue";
import ToggleSwitch from "@/modules/components/ui/ToggleSwitch.vue";
import {
  cardClass,
  inputClass,
  selectClass,
} from "@/modules/components/ui/form";
import { useI18n } from "@/modules/locales";
import { useLogStore } from "@/modules/stores/logs";
import { useUiStore } from "@/modules/stores/ui";
import { formatDateTime, formatTimeOnly } from "@/modules/utils/format";
import type { BadgeVariant } from "@/modules/utils/status";

type LevelFilter = "all" | LogLevel;

const logStore = useLogStore();
const ui = useUiStore();
const { t } = useI18n();

const level = ref<LevelFilter>("all");
const search = ref("");
const autoScroll = ref(true);
const expanded = ref<Set<number>>(new Set());
const listEl = ref<HTMLElement | null>(null);
const clearOpen = ref(false);
const clearBusy = ref(false);
const exporting = ref(false);

const levelOptions = computed<{ value: LevelFilter; label: string }[]>(() => [
  { value: "all", label: t("logs.levels.all") },
  { value: "debug", label: t("logs.level.debug") },
  { value: "info", label: t("logs.level.info") },
  { value: "warn", label: t("logs.level.warn") },
  { value: "error", label: t("logs.level.error") },
  { value: "success", label: t("logs.level.success") },
]);

const badgeVariants: Record<LogLevel, BadgeVariant> = {
  debug: "neutral",
  info: "info",
  warn: "warn",
  error: "danger",
  success: "success",
};

const borderClasses: Record<LogLevel, string> = {
  debug: "border-l-zinc-300 dark:border-l-zinc-700",
  info: "border-l-sky-300 dark:border-l-sky-600",
  warn: "border-l-amber-400 dark:border-l-amber-500",
  error: "border-l-red-400 dark:border-l-red-500",
  success: "border-l-emerald-400 dark:border-l-emerald-500",
};

const messageClasses: Record<LogLevel, string> = {
  debug: "text-zinc-500 dark:text-zinc-400",
  info: "text-zinc-700 dark:text-zinc-200",
  warn: "text-amber-700 dark:text-amber-300",
  error: "text-red-700 dark:text-red-300",
  success: "text-emerald-700 dark:text-emerald-300",
};

const keyword = computed(() => search.value.trim().toLowerCase());

const filtered = computed<LogEntry[]>(() => {
  const words = keyword.value.split(/\s+/).filter(Boolean);
  return logStore.entries.filter((entry) => {
    if (level.value !== "all" && entry.level !== level.value) return false;
    if (words.length === 0) return true;
    const haystack = [entry.scope, entry.message, entry.detail ?? ""]
      .join(" ")
      .toLowerCase();
    return words.every((word) => haystack.includes(word));
  });
});

function isExpanded(id: number) {
  return expanded.value.has(id);
}

function toggleDetail(id: number) {
  const next = new Set(expanded.value);
  if (next.has(id)) {
    next.delete(id);
  } else {
    next.add(id);
  }
  expanded.value = next;
}

function scrollToBottom() {
  const el = listEl.value;
  if (el) el.scrollTop = el.scrollHeight;
}

async function maybeScroll() {
  await nextTick();
  if (autoScroll.value) scrollToBottom();
}

watch(
  () => logStore.entries.length,
  () => {
    void maybeScroll();
  },
);

watch(autoScroll, (enabled) => {
  if (enabled) void maybeScroll();
});

async function exportLogs() {
  exporting.value = true;
  try {
    const path = await save({
      title: t("logs.export"),
      defaultPath: "daemon-man-logs.txt",
      filters: [{ name: "Text", extensions: ["txt", "log"] }],
    });
    if (!path || typeof path !== "string") return;
    await logStore.exportTo(path);
    ui.notify("success", t("logs.exportSuccess"));
  } catch (error) {
    ui.notify("error", t("logs.exportFailed", { error: String(error) }));
  } finally {
    exporting.value = false;
  }
}

async function confirmClear() {
  clearBusy.value = true;
  try {
    await logStore.clear();
    clearOpen.value = false;
    expanded.value = new Set();
    await nextTick();
    scrollToBottom();
  } catch (error) {
    ui.notify("error", t("error.invoke", { error: String(error) }));
  } finally {
    clearBusy.value = false;
  }
}

async function reload() {
  try {
    await logStore.load();
    await maybeScroll();
  } catch (error) {
    ui.notify("error", t("error.invoke", { error: String(error) }));
  }
}

onMounted(async () => {
  if (!logStore.loaded) {
    await reload();
  } else {
    await maybeScroll();
  }
});
</script>

<template>
  <div class="flex h-full min-h-[420px] flex-col">
    <PageHeader :title="t('logs.title')" :description="t('logs.subtitle')">
      <template #actions>
        <BaseButton
          variant="secondary"
          size="sm"
          :loading="logStore.loading"
          :disabled="logStore.loading"
          @click="reload"
        >
          <RefreshCw v-if="!logStore.loading" class="h-3.5 w-3.5" />
          {{ t("common.refresh") }}
        </BaseButton>
        <BaseButton
          variant="secondary"
          size="sm"
          :loading="exporting"
          @click="exportLogs"
        >
          <Download v-if="!exporting" class="h-3.5 w-3.5" />
          {{ t("logs.export") }}
        </BaseButton>
        <BaseButton
          variant="danger"
          size="sm"
          :disabled="logStore.entries.length === 0"
          @click="clearOpen = true"
        >
          <Trash2 class="h-3.5 w-3.5" />
          {{ t("logs.clear") }}
        </BaseButton>
      </template>
    </PageHeader>

    <div :class="[cardClass, 'flex min-h-0 flex-1 flex-col overflow-hidden']">
      <div
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
            :placeholder="t('common.search')"
          />
        </div>

        <div class="relative min-w-[9rem] shrink-0">
          <select v-model="level" :class="selectClass">
            <option
              v-for="option in levelOptions"
              :key="option.value"
              :value="option.value"
            >
              {{ option.label }}
            </option>
          </select>
          <ChevronDown
            class="pointer-events-none absolute right-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-zinc-400"
          />
        </div>

        <label
          class="flex shrink-0 cursor-pointer items-center gap-2 rounded-lg border border-zinc-200 px-2.5 py-1.5 text-xs text-zinc-600 dark:border-zinc-700 dark:text-zinc-300"
        >
          <ToggleSwitch v-model="autoScroll" size="sm" />
          <span>{{ t("logs.autoScroll") }}</span>
        </label>

        <span class="shrink-0 text-[11px] text-zinc-500 dark:text-zinc-400">
          {{ t("logs.count", { count: filtered.length }) }}
        </span>
      </div>

      <div
        v-if="logStore.loading && logStore.entries.length === 0"
        class="flex flex-1 flex-col gap-2 p-4"
      >
        <div
          v-for="index in 8"
          :key="index"
          class="h-3.5 animate-pulse rounded bg-zinc-200 dark:bg-zinc-800"
          :class="index % 3 === 0 ? 'w-2/3' : 'w-full'"
        />
      </div>

      <EmptyState
        v-else-if="filtered.length === 0"
        class="flex-1"
        :title="t('logs.empty')"
        :description="t('logs.emptyHint')"
      />

      <div
        v-else
        ref="listEl"
        class="min-h-0 flex-1 overflow-auto"
      >
        <div class="min-w-[720px]">
          <div
            class="sticky top-0 z-10 grid grid-cols-[7rem_6rem_7rem_minmax(0,1fr)_1.75rem] items-center gap-2 border-b border-zinc-200/80 bg-white/95 px-4 py-2 text-[11px] font-semibold uppercase tracking-wider text-zinc-500 backdrop-blur dark:border-zinc-800/80 dark:bg-zinc-900/95 dark:text-zinc-400"
          >
            <span>{{ t("logs.time") }}</span>
            <span>{{ t("logs.level") }}</span>
            <span>{{ t("logs.scope") }}</span>
            <span>{{ t("logs.message") }}</span>
            <span />
          </div>

          <div
            class="divide-y divide-zinc-200/70 dark:divide-zinc-800/70"
          >
            <div
              v-for="entry in filtered"
              :key="entry.id"
              class="border-l-2 px-4 py-2 transition hover:bg-zinc-50 dark:hover:bg-zinc-800/30"
              :class="borderClasses[entry.level]"
            >
              <div
                class="grid grid-cols-[7rem_6rem_7rem_minmax(0,1fr)_1.75rem] items-start gap-2"
              >
                <span
                  class="font-mono text-[11px] leading-5 text-zinc-500 dark:text-zinc-400"
                  :title="formatDateTime(entry.ts)"
                >
                  {{ formatTimeOnly(entry.ts) }}
                </span>
                <span>
                  <BaseBadge :variant="badgeVariants[entry.level]">
                    {{ t(`logs.level.${entry.level}`) }}
                  </BaseBadge>
                </span>
                <span
                  class="truncate font-mono text-[11px] leading-5 text-zinc-500 dark:text-zinc-400"
                  :title="entry.scope"
                >
                  {{ entry.scope }}
                </span>
                <p
                  class="min-w-0 whitespace-pre-wrap break-words text-xs leading-5"
                  :class="messageClasses[entry.level]"
                  :title="entry.detail ?? entry.message"
                >
                  {{ entry.message }}
                </p>
                <button
                  v-if="entry.detail"
                  type="button"
                  class="mt-0.5 flex h-5 w-5 items-center justify-center rounded text-zinc-400 transition hover:bg-zinc-200 hover:text-zinc-600 dark:hover:bg-zinc-700 dark:hover:text-zinc-200"
                  :title="entry.detail"
                  @click="toggleDetail(entry.id)"
                >
                  <ChevronDown v-if="isExpanded(entry.id)" class="h-3.5 w-3.5" />
                  <ChevronRight v-else class="h-3.5 w-3.5" />
                </button>
                <span v-else />
              </div>

              <pre
                v-if="entry.detail && isExpanded(entry.id)"
                class="mt-1.5 ml-[7rem] max-h-64 overflow-auto whitespace-pre-wrap break-words rounded-lg bg-zinc-100 px-3 py-2 font-mono text-[11px] leading-relaxed text-zinc-600 dark:bg-zinc-950/60 dark:text-zinc-300"
              >{{ entry.detail }}</pre>
            </div>
          </div>
        </div>
      </div>
    </div>

    <BaseModal
      :open="clearOpen"
      :title="t('logs.clear')"
      size="sm"
      :closable="!clearBusy"
      @close="clearOpen = false"
    >
      <p class="text-xs leading-relaxed text-zinc-600 dark:text-zinc-300">
        {{ t("logs.clearConfirm") }}
      </p>
      <template #footer>
        <BaseButton
          variant="secondary"
          size="sm"
          :disabled="clearBusy"
          @click="clearOpen = false"
        >
          {{ t("common.cancel") }}
        </BaseButton>
        <BaseButton
          variant="danger"
          size="sm"
          :loading="clearBusy"
          @click="confirmClear"
        >
          {{ t("logs.clear") }}
        </BaseButton>
      </template>
    </BaseModal>
  </div>
</template>
