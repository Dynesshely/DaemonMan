<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { useRouter } from "vue-router";
import {
  Check,
  Filter,
  RefreshCw,
  Search,
  Settings,
  ShieldAlert,
  Users,
} from "lucide-vue-next";

import type { AutostartItem, TakeOverResult } from "@/api";
import PageHeader from "@/modules/components/layout/PageHeader.vue";
import BaseButton from "@/modules/components/ui/BaseButton.vue";
import BaseModal from "@/modules/components/ui/BaseModal.vue";
import EmptyState from "@/modules/components/ui/EmptyState.vue";
import SectionCard from "@/modules/components/ui/SectionCard.vue";
import StartupItemRow from "@/modules/components/startup/StartupItemRow.vue";
import TakeOverModal from "@/modules/components/startup/TakeOverModal.vue";
import { inputClass } from "@/modules/components/ui/form";
import { useI18n } from "@/modules/locales";
import { useAppStore } from "@/modules/stores/app";
import { useAutostartStore } from "@/modules/stores/autostart";
import { useUiStore } from "@/modules/stores/ui";
import { useWorkflowStore } from "@/modules/stores/workflows";
import { formatRelative } from "@/modules/utils/format";

type FilterKind = "all" | "enabled" | "disabled" | "takenOver";

const router = useRouter();
const appStore = useAppStore();
const autostartStore = useAutostartStore();
const workflowStore = useWorkflowStore();
const ui = useUiStore();
const { t, locale } = useI18n();

const search = ref("");
const filter = ref<FilterKind>("all");
const selected = ref<Set<string>>(new Set());
const pending = ref<Set<string>>(new Set());
const takeOverOpen = ref(false);
const takeOverBusy = ref(false);
const takeOverResult = ref<TakeOverResult | null>(null);
const releaseTarget = ref<AutostartItem | null>(null);
const releaseBusy = ref(false);

const filters = computed<
  { value: FilterKind; label: string }[]
>(() => [
  { value: "all", label: t("startup.filter.all") },
  { value: "enabled", label: t("startup.filter.enabled") },
  { value: "disabled", label: t("startup.filter.disabled") },
  { value: "takenOver", label: t("startup.filter.takenOver") },
]);

const keyword = computed(() => search.value.trim().toLowerCase());

const filtered = computed<AutostartItem[]>(() => {
  const words = keyword.value.split(/\s+/).filter(Boolean);
  return autostartStore.items.filter((item) => {
    if (filter.value === "enabled" && !item.enabled) return false;
    if (filter.value === "disabled" && item.enabled) return false;
    if (filter.value === "takenOver" && !item.takenOver) return false;
    if (words.length === 0) return true;
    const haystack = [
      item.name,
      item.command,
      item.sourceLabel,
      item.locator,
      item.detail ?? "",
      item.executable ?? "",
      item.args ?? "",
    ]
      .join(" ")
      .toLowerCase();
    return words.every((word) => haystack.includes(word));
  });
});

const selectableItems = computed(() =>
  filtered.value.filter((item) => !item.isSelf && !item.takenOver),
);

const allSelected = computed(
  () =>
    selectableItems.value.length > 0 &&
    selectableItems.value.every((item) => selected.value.has(item.id)),
);

const selectedItems = computed(() =>
  autostartStore.items.filter((item) => selected.value.has(item.id)),
);

const elevated = computed(() => appStore.systemInfo?.elevated ?? false);

const lastScan = computed(() => {
  if (!autostartStore.scannedAt) return null;
  return formatRelative(
    new Date(autostartStore.scannedAt).toISOString(),
    locale.value,
  );
});

const showSkeleton = computed(
  () => autostartStore.scanning && autostartStore.items.length === 0,
);

function isSelected(itemId: string) {
  return selected.value.has(itemId);
}

function setSelected(itemId: string, value: boolean) {
  const next = new Set(selected.value);
  if (value) {
    next.add(itemId);
  } else {
    next.delete(itemId);
  }
  selected.value = next;
}

function clearSelection() {
  selected.value = new Set();
}

function toggleSelectAll() {
  if (allSelected.value) {
    clearSelection();
    return;
  }
  selected.value = new Set(selectableItems.value.map((item) => item.id));
}

async function rescan() {
  try {
    await autostartStore.scan(true);
    if (autostartStore.error) {
      ui.notify("error", String(autostartStore.error));
    }
  } catch (error) {
    ui.notify("error", String(error));
  }
}

async function onToggle(item: AutostartItem, enabled: boolean) {
  const next = new Set(pending.value);
  next.add(item.id);
  pending.value = next;
  try {
    await autostartStore.toggle(item.id, enabled);
  } catch (error) {
    ui.notify("error", t("startup.toggleFailed", { error: String(error) }));
  } finally {
    const done = new Set(pending.value);
    done.delete(item.id);
    pending.value = done;
  }
}

function openTakeOver() {
  if (selectedItems.value.length === 0) {
    ui.notify("info", t("startup.noTakeOver"));
    return;
  }
  takeOverResult.value = null;
  takeOverOpen.value = true;
}

function closeTakeOver() {
  if (takeOverBusy.value) return;
  takeOverOpen.value = false;
  takeOverResult.value = null;
}

async function confirmTakeOver(payload: {
  workflowId?: string;
  workflowName?: string;
}) {
  const ids = selectedItems.value.map((item) => item.id);
  if (ids.length === 0) {
    takeOverOpen.value = false;
    return;
  }
  takeOverBusy.value = true;
  try {
    const result = await autostartStore.takeOver(ids, payload);
    takeOverResult.value = result;
    await workflowStore.load();
    const succeeded = new Set(result.items.map((item) => item.id));
    selected.value = new Set(
      [...selected.value].filter((id) => !succeeded.has(id)),
    );
    ui.notify(
      "success",
      t("startup.takeOverSuccess", { count: result.items.length }),
    );
    if (result.failures.length === 0 && result.skipped.length === 0) {
      takeOverOpen.value = false;
      takeOverResult.value = null;
      clearSelection();
    }
  } catch (error) {
    ui.notify("error", t("startup.takeOverFailed", { error: String(error) }));
  } finally {
    takeOverBusy.value = false;
  }
}

function askRelease(item: AutostartItem) {
  releaseTarget.value = item;
}

function closeRelease() {
  if (releaseBusy.value) return;
  releaseTarget.value = null;
}

async function confirmRelease() {
  const target = releaseTarget.value;
  if (!target) return;
  releaseBusy.value = true;
  try {
    await autostartStore.release(target.id);
    ui.notify("success", t("startup.releaseSuccess"));
    releaseTarget.value = null;
  } catch (error) {
    ui.notify("error", t("error.invoke", { error: String(error) }));
  } finally {
    releaseBusy.value = false;
  }
}

function goSettings() {
  void router.push("/settings");
}

watch(
  () => autostartStore.items,
  (items) => {
    const ids = new Set(items.map((item) => item.id));
    const next = new Set(
      [...selected.value].filter((id) => ids.has(id)),
    );
    if (next.size !== selected.value.size) {
      selected.value = next;
    }
  },
);

onMounted(async () => {
  await Promise.allSettled([
    appStore.refreshSystemInfo(),
    autostartStore.scan(),
    workflowStore.workflows.length === 0
      ? workflowStore.load()
      : Promise.resolve(),
  ]);
});
</script>

<template>
  <div>
    <PageHeader
      :title="t('startup.title')"
      :description="t('startup.subtitle')"
    />

    <div
      v-if="appStore.systemInfo && !elevated"
      class="mb-3 flex flex-wrap items-center gap-2 rounded-xl border border-amber-200 bg-amber-50/80 px-3.5 py-2.5 text-xs text-amber-900 dark:border-amber-500/30 dark:bg-amber-500/10 dark:text-amber-200"
    >
      <ShieldAlert class="h-4 w-4 shrink-0" />
      <span class="min-w-0 flex-1 leading-relaxed">
        {{ t("startup.adminHint") }}
      </span>
      <BaseButton variant="secondary" size="sm" @click="goSettings">
        <Settings class="h-3.5 w-3.5" />
        {{ t("nav.settings") }}
      </BaseButton>
    </div>

    <div
      v-if="autostartStore.error"
      class="mb-3 flex flex-wrap items-center gap-2 rounded-xl border border-red-200 bg-red-50/80 px-3.5 py-2.5 text-xs text-red-800 dark:border-red-500/30 dark:bg-red-500/10 dark:text-red-200"
    >
      <span class="min-w-0 flex-1 leading-relaxed">
        {{ t("startup.error", { error: autostartStore.error ?? "" }) }}
      </span>
      <BaseButton variant="secondary" size="sm" @click="rescan">
        <RefreshCw class="h-3.5 w-3.5" />
        {{ t("common.retry") }}
      </BaseButton>
    </div>

    <SectionCard :padded="false" dense>
      <div class="flex flex-col gap-2.5 border-b border-zinc-200/80 p-3 dark:border-zinc-800/80 lg:flex-row lg:items-center">
        <div class="relative min-w-0 flex-1">
          <Search
            class="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-zinc-400"
          />
          <input
            v-model="search"
            :class="[inputClass, 'pl-9']"
            type="search"
            :placeholder="t('startup.searchPlaceholder')"
          />
        </div>

        <div
          class="flex shrink-0 items-center gap-1 rounded-lg border border-zinc-200 bg-zinc-50 p-0.5 dark:border-zinc-700 dark:bg-zinc-900"
        >
          <button
            v-for="option in filters"
            :key="option.value"
            type="button"
            class="rounded-md px-2.5 py-1 text-[11px] font-medium transition"
            :class="
              filter === option.value
                ? 'bg-white text-zinc-900 shadow-sm dark:bg-zinc-700 dark:text-zinc-100'
                : 'text-zinc-500 hover:text-zinc-800 dark:text-zinc-400 dark:hover:text-zinc-200'
            "
            @click="filter = option.value"
          >
            {{ option.label }}
          </button>
        </div>

        <div class="flex shrink-0 items-center gap-1.5">
          <span
            v-if="selectedItems.length"
            class="hidden text-[11px] text-brand-600 dark:text-brand-400 sm:inline"
          >
            {{ t("startup.selected", { count: selectedItems.length }) }}
          </span>
          <BaseButton
            variant="ghost"
            size="sm"
            :loading="autostartStore.scanning"
            :disabled="autostartStore.scanning"
            @click="rescan"
          >
            <RefreshCw v-if="!autostartStore.scanning" class="h-3.5 w-3.5" />
            {{ t("startup.rescan") }}
          </BaseButton>
          <BaseButton variant="ghost" size="sm" @click="toggleSelectAll">
            <Users class="h-3.5 w-3.5" />
            {{
              allSelected ? t("startup.clearSelection") : t("startup.selectAll")
            }}
          </BaseButton>
          <BaseButton
            variant="primary"
            size="sm"
            :disabled="selectedItems.length === 0"
            @click="openTakeOver"
          >
            <Check class="h-3.5 w-3.5" />
            {{ t("startup.takeOver") }}
            <span v-if="selectedItems.length">· {{ selectedItems.length }}</span>
          </BaseButton>
        </div>
      </div>

      <div
        class="flex flex-wrap items-center gap-x-3 gap-y-1 border-b border-zinc-200/70 px-4 py-2 text-[11px] text-zinc-500 dark:border-zinc-800/70 dark:text-zinc-400"
      >
        <span class="inline-flex items-center gap-1">
          <Filter class="h-3 w-3" />
          {{ t("startup.totalCount", { count: filtered.length }) }}
        </span>
        <span v-if="selectedItems.length" class="text-brand-600 dark:text-brand-400">
          {{ t("startup.selected", { count: selectedItems.length }) }}
        </span>
        <span v-if="lastScan" class="ml-auto">
          {{ t("startup.lastScan", { time: lastScan }) }}
        </span>
      </div>

      <div v-if="showSkeleton" class="flex flex-col gap-3 p-4">
        <div
          v-for="index in 6"
          :key="index"
          class="flex items-center gap-3"
        >
          <div class="h-4 w-4 animate-pulse rounded bg-zinc-200 dark:bg-zinc-800" />
          <div class="flex-1 space-y-2">
            <div class="h-3 w-1/3 animate-pulse rounded bg-zinc-200 dark:bg-zinc-800" />
            <div class="h-3 w-2/3 animate-pulse rounded bg-zinc-200 dark:bg-zinc-800" />
          </div>
        </div>
      </div>

      <div
        v-else-if="filtered.length"
        class="divide-y divide-zinc-200/80 dark:divide-zinc-800/80"
      >
        <StartupItemRow
          v-for="item in filtered"
          :key="item.id"
          :item="item"
          :selected="isSelected(item.id)"
          :selectable="!item.isSelf && !item.takenOver"
          :pending="pending.has(item.id)"
          @update:selected="setSelected(item.id, $event)"
          @toggle="onToggle"
          @release="askRelease"
        />
      </div>

      <EmptyState
        v-else
        :title="t('startup.empty')"
        :description="t('startup.emptyHint')"
      >
        <template #action>
          <BaseButton variant="subtle" size="sm" @click="rescan">
            <RefreshCw class="h-3.5 w-3.5" />
            {{ t("startup.rescan") }}
          </BaseButton>
        </template>
      </EmptyState>
    </SectionCard>

    <TakeOverModal
      :open="takeOverOpen"
      :count="selectedItems.length"
      :workflows="workflowStore.workflows"
      :busy="takeOverBusy"
      :result="takeOverResult"
      @close="closeTakeOver"
      @confirm="confirmTakeOver"
    />

    <BaseModal
      :open="releaseTarget !== null"
      :title="t('startup.release')"
      size="sm"
      :closable="!releaseBusy"
      @close="closeRelease"
    >
      <p class="text-xs leading-relaxed text-zinc-600 dark:text-zinc-300">
        {{
          t("startup.releaseConfirm", { name: releaseTarget?.name ?? "" })
        }}
      </p>
      <template #footer>
        <BaseButton
          variant="secondary"
          size="sm"
          :disabled="releaseBusy"
          @click="closeRelease"
        >
          {{ t("common.cancel") }}
        </BaseButton>
        <BaseButton
          variant="danger"
          size="sm"
          :loading="releaseBusy"
          @click="confirmRelease"
        >
          {{ t("startup.release") }}
        </BaseButton>
      </template>
    </BaseModal>
  </div>
</template>
