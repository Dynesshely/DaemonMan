<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { useRouter } from "vue-router";
import {
  Check,
  Filter,
  Layers,
  MonitorSmartphone,
  RefreshCw,
  Search,
  Settings,
  ShieldAlert,
  Users,
} from "lucide-vue-next";

import type { AutostartItem, TakeOverResult } from "@/api";
import { api } from "@/api";
import PageHeader from "@/modules/components/layout/PageHeader.vue";
import BaseButton from "@/modules/components/ui/BaseButton.vue";
import BaseModal from "@/modules/components/ui/BaseModal.vue";
import EmptyState from "@/modules/components/ui/EmptyState.vue";
import SectionCard from "@/modules/components/ui/SectionCard.vue";
import FilterTabs from "@/modules/components/startup/FilterTabs.vue";
import StartupContextMenu from "@/modules/components/startup/StartupContextMenu.vue";
import StartupItemRow from "@/modules/components/startup/StartupItemRow.vue";
import TakeOverModal from "@/modules/components/startup/TakeOverModal.vue";
import {
  CATEGORY_VALUES,
  LOCATION_VALUES,
  categoryOf,
  locationOf,
  type CategoryFilter,
  type LocationFilter,
} from "@/modules/components/startup/categories";
import { inputClass } from "@/modules/components/ui/form";
import { useI18n } from "@/modules/locales";
import { useAppStore } from "@/modules/stores/app";
import { useAutostartStore } from "@/modules/stores/autostart";
import { useIconStore } from "@/modules/stores/icons";
import { useUiStore } from "@/modules/stores/ui";
import { useWorkflowStore } from "@/modules/stores/workflows";
import { formatRelative } from "@/modules/utils/format";

type FilterKind = "all" | "enabled" | "disabled" | "takenOver";

const router = useRouter();
const appStore = useAppStore();
const autostartStore = useAutostartStore();
const iconStore = useIconStore();
const workflowStore = useWorkflowStore();
const ui = useUiStore();
const { t, locale } = useI18n();

const search = ref("");
const filter = ref<FilterKind>("all");
const category = ref<CategoryFilter>("all");
const location = ref<LocationFilter>("all");
const selected = ref<Set<string>>(new Set());
const pending = ref<Set<string>>(new Set());
const takeOverOpen = ref(false);
const takeOverBusy = ref(false);
const takeOverResult = ref<TakeOverResult | null>(null);
const releaseTarget = ref<AutostartItem | null>(null);
const releaseBusy = ref(false);
const contextOpen = ref(false);
const contextX = ref(0);
const contextY = ref(0);
const contextTarget = ref<AutostartItem | null>(null);
const disableTarget = ref<AutostartItem | null>(null);
const disableBusy = ref(false);
const deleteTarget = ref<AutostartItem | null>(null);
const deleteBusy = ref(false);
const restoreBusy = ref(false);

const actionBusy = computed(
  () => disableBusy.value || deleteBusy.value || restoreBusy.value,
);

const filters = computed<
  { value: FilterKind; label: string }[]
>(() => [
  { value: "all", label: t("startup.filter.all") },
  { value: "enabled", label: t("startup.filter.enabled") },
  { value: "disabled", label: t("startup.filter.disabled") },
  { value: "takenOver", label: t("startup.filter.takenOver") },
]);

const keyword = computed(() => search.value.trim().toLowerCase());

const SOURCE_LABEL_KEYS: Record<AutostartItem["source"], string> = {
  "registry-run": "startup.source.registry-run",
  "registry-run-once": "startup.source.registry-run-once",
  "startup-folder": "startup.source.startup-folder",
  "scheduled-task": "startup.source.scheduled-task",
  service: "startup.source.service",
};

function sourceLabelOf(source: AutostartItem["source"]) {
  return t(SOURCE_LABEL_KEYS[source] ?? "startup.source.registry-run");
}

/**
 * 「备份后移除」的项不在系统扫描结果里，但从 overrides 里补一行「已移除」，
 * 这样仍能通过右键菜单恢复。
 */
const removedItems = computed<AutostartItem[]>(() => {
  const present = new Set(autostartStore.items.map((item) => item.id));
  const overrides = appStore.config?.overrides ?? [];
  return overrides
    .filter((override) => override.mode === "removed" && !present.has(override.id))
    .map((override) => ({
      id: override.id,
      name: override.name,
      source: override.source,
      sourceLabel: sourceLabelOf(override.source),
      command: override.command || "",
      executable: null,
      args: null,
      enabled: false,
      toggleSupported: false,
      canDelete: false,
      requiresAdmin: false,
      isSelf: false,
      takenOver: true,
      scope: override.scope || "user",
      locator: "",
      detail: t("startup.removedDetail"),
      removed: true,
    }));
});

const allItems = computed<AutostartItem[]>(() => [
  ...autostartStore.items,
  ...removedItems.value,
]);

const filtered = computed<AutostartItem[]>(() => {
  const words = keyword.value.split(/\s+/).filter(Boolean);
  return allItems.value.filter((item) => {
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

/** 状态筛选 + 搜索后的基础结果（不含类别/位置 Tab）。 */
const visible = computed<AutostartItem[]>(() =>
  filtered.value.filter(
    (item) =>
      (category.value === "all" || categoryOf(item) === category.value) &&
      (location.value === "all" || locationOf(item) === location.value),
  ),
);

/** 类别 Tab 数量：基于「当前位置 Tab + 搜索 + 状态筛选」的结果。 */
const categoryOptions = computed(() => {
  const base = filtered.value.filter(
    (item) => location.value === "all" || locationOf(item) === location.value,
  );
  const countOf = (value: CategoryFilter) =>
    value === "all"
      ? base.length
      : base.filter((item) => categoryOf(item) === value).length;
  return CATEGORY_VALUES.map((value) => ({
    value,
    label: t(`startup.category.${value}`),
    count: countOf(value),
  }));
});

/** 位置 Tab 数量：基于「当前类别 Tab + 搜索 + 状态筛选」的结果。 */
const locationOptions = computed(() => {
  const base = filtered.value.filter(
    (item) => category.value === "all" || categoryOf(item) === category.value,
  );
  const countOf = (value: LocationFilter) =>
    value === "all"
      ? base.length
      : base.filter((item) => locationOf(item) === value).length;
  return LOCATION_VALUES.map((value) => ({
    value,
    label: t(`startup.location.${value}`),
    count: countOf(value),
  }));
});

const categoryModel = computed({
  get: () => category.value as string,
  set: (value: string) => {
    if ((CATEGORY_VALUES as string[]).includes(value)) {
      category.value = value as CategoryFilter;
    }
  },
});

const locationModel = computed({
  get: () => location.value as string,
  set: (value: string) => {
    if ((LOCATION_VALUES as string[]).includes(value)) {
      location.value = value as LocationFilter;
    }
  },
});

const selectableItems = computed(() =>
  visible.value.filter((item) => !item.isSelf && !item.takenOver),
);

const allSelected = computed(
  () =>
    selectableItems.value.length > 0 &&
    selectableItems.value.every((item) => selected.value.has(item.id)),
);

/** 只有当前可见行的选中项参与批量接管。 */
const selectedItems = computed(() =>
  visible.value.filter((item) => selected.value.has(item.id)),
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
  } finally {
    syncIcons();
  }
}

/** 扫描结果变化后补齐行内图标（去重、批量、失败静默）。 */
function syncIcons() {
  const paths = allItems.value
    .map((item) => item.executable)
    .filter((path): path is string => Boolean(path));
  if (paths.length > 0) {
    void iconStore.ensure(paths);
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
    await appStore.refreshConfig();
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

function openContextMenu(item: AutostartItem, event: MouseEvent) {
  contextTarget.value = item;
  contextX.value = event.clientX;
  contextY.value = event.clientY;
  contextOpen.value = true;
}

function closeContextMenu() {
  contextOpen.value = false;
  contextTarget.value = null;
}

/** 右键「禁用」：先确认，再交给后端（系统开关或备份后移除）。 */
function askDisable(item: AutostartItem) {
  closeContextMenu();
  if (!item.enabled || item.isSelf) return;
  disableTarget.value = item;
}

function closeDisable() {
  if (disableBusy.value) return;
  disableTarget.value = null;
}

async function confirmDisable() {
  const target = disableTarget.value;
  if (!target || disableBusy.value) return;
  disableBusy.value = true;
  try {
    await api.disableAutostartItem(target.id);
    await appStore.refreshConfig();
    await autostartStore.scan(true);
    syncIcons();
    ui.notify("success", t("startup.disableSuccess", { name: target.name }));
    disableTarget.value = null;
  } catch (error) {
    ui.notify("error", t("startup.disableFailed", { error: String(error) }));
  } finally {
    disableBusy.value = false;
  }
}

/** 右键「恢复」：无需确认。 */
async function restoreItem(item: AutostartItem) {
  closeContextMenu();
  if (item.isSelf || restoreBusy.value) return;
  if (!(item.enabled === false || item.takenOver)) return;
  restoreBusy.value = true;
  try {
    await api.restoreAutostartItem(item.id);
    await appStore.refreshConfig();
    await autostartStore.scan(true);
    syncIcons();
    ui.notify("success", t("startup.restoreSuccess", { name: item.name }));
  } catch (error) {
    ui.notify("error", t("startup.restoreFailed", { error: String(error) }));
  } finally {
    restoreBusy.value = false;
  }
}

/** 右键「删除」：先确认，再备份到配置目录并从原位置删除。 */
function askDelete(item: AutostartItem) {
  closeContextMenu();
  if (!item.canDelete || item.isSelf) return;
  deleteTarget.value = item;
}

function closeDelete() {
  if (deleteBusy.value) return;
  deleteTarget.value = null;
}

async function confirmDelete() {
  const target = deleteTarget.value;
  if (!target || deleteBusy.value) return;
  deleteBusy.value = true;
  try {
    await api.deleteAutostartItem(target.id);
    await appStore.refreshConfig();
    await autostartStore.scan(true);
    syncIcons();
    ui.notify("success", t("startup.deleteSuccess", { name: target.name }));
    deleteTarget.value = null;
  } catch (error) {
    ui.notify("error", t("startup.deleteFailed", { error: String(error) }));
  } finally {
    deleteBusy.value = false;
  }
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
    syncIcons();
    if (
      contextTarget.value &&
      !items.some((item) => item.id === contextTarget.value?.id)
    ) {
      closeContextMenu();
    }
  },
);

// 切换类别/位置 Tab 时丢弃不可见行的选中状态，避免批量接管误伤。
watch([category, location], () => {
  const visibleIds = new Set(visible.value.map((item) => item.id));
  const next = new Set(
    [...selected.value].filter((id) => visibleIds.has(id)),
  );
  if (next.size !== selected.value.size) {
    selected.value = next;
  }
});

onMounted(async () => {
  await Promise.allSettled([
    appStore.refreshSystemInfo(),
    autostartStore.scan(),
    workflowStore.workflows.length === 0
      ? workflowStore.load()
      : Promise.resolve(),
  ]);
  syncIcons();
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
        class="flex flex-col gap-2 border-b border-zinc-200/80 px-3 py-2.5 dark:border-zinc-800/80"
      >
        <div class="flex flex-wrap items-center gap-2">
          <span
            class="inline-flex shrink-0 items-center gap-1 text-[11px] font-semibold uppercase tracking-wider text-zinc-500 dark:text-zinc-400"
          >
            <Layers class="h-3 w-3" />
            {{ t("startup.tabs.category") }}
          </span>
          <FilterTabs v-model="categoryModel" :options="categoryOptions" />
        </div>
        <div class="flex flex-wrap items-center gap-2">
          <span
            class="inline-flex shrink-0 items-center gap-1 text-[11px] font-semibold uppercase tracking-wider text-zinc-500 dark:text-zinc-400"
          >
            <MonitorSmartphone class="h-3 w-3" />
            {{ t("startup.tabs.location") }}
          </span>
          <FilterTabs v-model="locationModel" :options="locationOptions" />
        </div>
      </div>

      <div
        class="flex flex-wrap items-center gap-x-3 gap-y-1 border-b border-zinc-200/70 px-4 py-2 text-[11px] text-zinc-500 dark:border-zinc-800/70 dark:text-zinc-400"
      >
        <span class="inline-flex items-center gap-1">
          <Filter class="h-3 w-3" />
          {{ t("startup.totalCount", { count: visible.length }) }}
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
        v-else-if="visible.length"
        class="divide-y divide-zinc-200/80 dark:divide-zinc-800/80"
      >
        <StartupItemRow
          v-for="item in visible"
          :key="item.id"
          :item="item"
          :selected="isSelected(item.id)"
          :selectable="!item.isSelf && !item.takenOver"
          :pending="pending.has(item.id)"
          @update:selected="setSelected(item.id, $event)"
          @toggle="onToggle"
          @release="askRelease"
          @contextmenu="openContextMenu"
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

    <StartupContextMenu
      :open="contextOpen"
      :x="contextX"
      :y="contextY"
      :item="contextTarget"
      :busy="actionBusy"
      @close="closeContextMenu"
      @disable="askDisable"
      @restore="restoreItem"
      @delete="askDelete"
    />

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

    <!-- 右键「禁用」确认 -->
    <BaseModal
      :open="disableTarget !== null"
      :title="t('startup.menu.disable')"
      size="sm"
      :closable="!disableBusy"
      @close="closeDisable"
    >
      <p class="text-xs leading-relaxed text-zinc-600 dark:text-zinc-300">
        {{
          t("startup.disableConfirm", { name: disableTarget?.name ?? "" })
        }}
      </p>
      <template #footer>
        <BaseButton
          variant="secondary"
          size="sm"
          :disabled="disableBusy"
          @click="closeDisable"
        >
          {{ t("common.cancel") }}
        </BaseButton>
        <BaseButton
          variant="primary"
          size="sm"
          :loading="disableBusy"
          @click="confirmDisable"
        >
          {{ t("startup.menu.disable") }}
        </BaseButton>
      </template>
    </BaseModal>

    <!-- 右键「删除」确认 -->
    <BaseModal
      :open="deleteTarget !== null"
      :title="t('startup.menu.delete')"
      size="sm"
      :closable="!deleteBusy"
      @close="closeDelete"
    >
      <p class="text-xs leading-relaxed text-zinc-600 dark:text-zinc-300">
        {{ t("startup.deleteConfirm", { name: deleteTarget?.name ?? "" }) }}
      </p>
      <template #footer>
        <BaseButton
          variant="secondary"
          size="sm"
          :disabled="deleteBusy"
          @click="closeDelete"
        >
          {{ t("common.cancel") }}
        </BaseButton>
        <BaseButton
          variant="danger"
          size="sm"
          :loading="deleteBusy"
          @click="confirmDelete"
        >
          {{ t("startup.menu.delete") }}
        </BaseButton>
      </template>
    </BaseModal>
  </div>
</template>
