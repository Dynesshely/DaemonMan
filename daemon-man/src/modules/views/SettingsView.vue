<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import type { Component } from "vue";
import { open, save } from "@tauri-apps/plugin-dialog";
import {
  ChevronDown,
  Download,
  FolderOpen,
  Globe,
  History,
  Info,
  Monitor,
  Moon,
  Palette,
  Power,
  RotateCcw,
  ShieldCheck,
  ShieldX,
  Sun,
  Upload,
  Workflow,
} from "lucide-vue-next";

import type { AppSettings, LegacyImportResult } from "@/api";
import { api } from "@/api";
import PageHeader from "@/modules/components/layout/PageHeader.vue";
import BaseBadge from "@/modules/components/ui/BaseBadge.vue";
import BaseButton from "@/modules/components/ui/BaseButton.vue";
import BaseModal from "@/modules/components/ui/BaseModal.vue";
import EmptyState from "@/modules/components/ui/EmptyState.vue";
import FormField from "@/modules/components/ui/FormField.vue";
import SectionCard from "@/modules/components/ui/SectionCard.vue";
import { selectClass } from "@/modules/components/ui/form";
import NumberSettingRow from "@/modules/components/settings/NumberSettingRow.vue";
import SettingSwitchRow from "@/modules/components/settings/SettingSwitchRow.vue";
import { useI18n } from "@/modules/locales";
import { useAppStore } from "@/modules/stores/app";
import { useThemeStore, type ThemeMode } from "@/modules/stores/theme";
import { useUiStore } from "@/modules/stores/ui";
import { useWorkflowStore } from "@/modules/stores/workflows";

const appStore = useAppStore();
const themeStore = useThemeStore();
const workflowStore = useWorkflowStore();
const ui = useUiStore();
const { t } = useI18n();

const pendingField = ref<string | null>(null);
const exportBusy = ref(false);
const importBusy = ref(false);
const resetBusy = ref(false);
const quitBusy = ref(false);
const importOpen = ref(false);
const resetOpen = ref(false);
const quitOpen = ref(false);
const pendingImportPath = ref<string | null>(null);
const legacyBusy = ref(false);
const legacyPath = ref<string | null>(null);
const legacyResult = ref<LegacyImportResult | null>(null);

const DEFAULT_SETTINGS: AppSettings = {
  language: "zh-CN",
  theme: "system",
  launchAtLogin: false,
  startMinimized: true,
  minimizeToTray: true,
  runWorkflowsOnStart: true,
  startupGraceMs: 3000,
  defaultNodeTimeoutMs: 30000,
  maxConcurrency: 8,
  maxRunHistory: 50,
  maxLogEntries: 2000,
  confirmTakeOver: true,
  confirmQuit: true,
};

const themeOptions = computed<
  { value: ThemeMode; label: string; icon: Component }[]
>(() => [
  { value: "system", label: t("settings.themeSystem"), icon: Monitor },
  { value: "light", label: t("settings.themeLight"), icon: Sun },
  { value: "dark", label: t("settings.themeDark"), icon: Moon },
]);

const elevated = computed(() => appStore.systemInfo?.elevated ?? false);
const version = computed(() => appStore.systemInfo?.appVersion ?? "0.1.0");

const stack = "Tauri 2 · Vue 3 · TypeScript · Tailwind CSS 4 · Pinia";

async function persist(patch: Partial<AppSettings>, field: string) {
  if (!appStore.settings) return;
  pendingField.value = field;
  try {
    await appStore.saveSettings(patch);
  } catch (error) {
    ui.notify("error", t("settings.saveFailed", { error: String(error) }));
  } finally {
    pendingField.value = null;
  }
}

function onLanguageChange(event: Event) {
  const value = (event.target as HTMLSelectElement).value === "en" ? "en" : "zh-CN";
  void persist({ language: value }, "language");
}

async function chooseTheme(mode: ThemeMode) {
  const previous = themeStore.mode;
  themeStore.setMode(mode);
  if (!appStore.settings) return;
  pendingField.value = "theme";
  try {
    await appStore.saveSettings({ theme: mode });
  } catch (error) {
    themeStore.setMode(previous);
    ui.notify("error", t("settings.saveFailed", { error: String(error) }));
  } finally {
    pendingField.value = null;
  }
}

async function onLaunchAtLogin(value: boolean) {
  pendingField.value = "launchAtLogin";
  try {
    await appStore.setLaunchAtLogin(value);
    ui.notify("success", t("settings.saveSuccess"));
  } catch (error) {
    ui.notify("error", t("settings.saveFailed", { error: String(error) }));
  } finally {
    pendingField.value = null;
  }
}

async function openDirectory(kind: "config" | "log") {
  try {
    if (kind === "config") {
      await api.openConfigDir();
    } else {
      await api.openLogDir();
    }
  } catch (error) {
    ui.notify("error", t("error.invoke", { error: String(error) }));
  }
}

async function exportConfig() {
  exportBusy.value = true;
  try {
    const path = await save({
      title: t("settings.exportConfig"),
      defaultPath: "daemon-man-config.json",
      filters: [{ name: "JSON", extensions: ["json"] }],
    });
    if (!path || typeof path !== "string") return;
    await appStore.exportConfig(path);
    ui.notify("success", t("settings.exportSuccess"));
  } catch (error) {
    ui.notify("error", t("error.invoke", { error: String(error) }));
  } finally {
    exportBusy.value = false;
  }
}

async function pickImport() {
  try {
    const picked = await open({
      title: t("settings.importConfig"),
      multiple: false,
      directory: false,
      filters: [{ name: "JSON", extensions: ["json"] }],
    });
    if (!picked || typeof picked !== "string") return;
    pendingImportPath.value = picked;
    importOpen.value = true;
  } catch (error) {
    ui.notify("error", t("error.invoke", { error: String(error) }));
  }
}

async function confirmImport() {
  if (!pendingImportPath.value) return;
  importBusy.value = true;
  try {
    await appStore.importConfig(pendingImportPath.value);
    await appStore.refreshConfig();
    importOpen.value = false;
    pendingImportPath.value = null;
    ui.notify("success", t("settings.importSuccess"));
  } catch (error) {
    ui.notify("error", t("error.invoke", { error: String(error) }));
  } finally {
    importBusy.value = false;
  }
}

async function importLegacy() {
  if (legacyBusy.value) return;
  legacyBusy.value = true;
  try {
    const picked = await open({
      title: t("settings.importLegacy"),
      multiple: false,
      directory: false,
      filters: [{ name: "JSON", extensions: ["json"] }],
    });
    if (!picked || typeof picked !== "string") return;
    legacyPath.value = picked;
    const result = await api.importLegacyProgressManager(picked);
    legacyResult.value = result;
    ui.notify(
      "success",
      t("settings.importLegacySuccess", {
        name: result.workflowName,
        nodes: result.nodeCount,
        edges: result.edgeCount,
      }),
    );
    if (result.warnings.length > 0) {
      ui.notify(
        "warn",
        t("settings.importLegacyWarnings", { count: result.warnings.length }),
      );
    }
    await workflowStore.load();
    await appStore.refreshSystemInfo();
  } catch (error) {
    ui.notify("error", t("settings.importLegacyFailed", { error: String(error) }));
  } finally {
    legacyBusy.value = false;
  }
}

async function confirmReset() {
  resetBusy.value = true;
  try {
    await api.saveSettings({ ...DEFAULT_SETTINGS });
    await appStore.refreshConfig();
    themeStore.setMode("system");
    ui.setLocale("zh-CN");
    resetOpen.value = false;
    ui.notify("success", t("settings.saveSuccess"));
  } catch (error) {
    ui.notify("error", t("settings.saveFailed", { error: String(error) }));
  } finally {
    resetBusy.value = false;
  }
}

async function confirmQuit() {
  quitBusy.value = true;
  try {
    await api.quitApp();
  } catch (error) {
    ui.notify("error", t("error.invoke", { error: String(error) }));
    quitBusy.value = false;
    quitOpen.value = false;
  }
}

onMounted(async () => {
  if (!appStore.settings) {
    try {
      await appStore.init();
    } catch (error) {
      ui.notify("error", t("error.invoke", { error: String(error) }));
    }
  }
  await appStore.refreshSystemInfo();
});
</script>

<template>
  <div>
    <PageHeader
      :title="t('settings.title')"
      :description="t('settings.subtitle')"
    >
      <template #actions>
        <BaseBadge variant="neutral">
          <Info class="h-3 w-3" />
          {{ t("settings.hint") }}
        </BaseBadge>
      </template>
    </PageHeader>

    <EmptyState
      v-if="!appStore.settings"
      :title="t('common.loading')"
      :description="t('settings.hint')"
    />

    <div v-else class="grid gap-4 lg:grid-cols-2">
      <!-- 常规 -->
      <SectionCard
        :title="t('settings.general')"
        :description="t('settings.generalDesc')"
      >
        <div class="flex flex-col gap-3">
          <FormField :label="t('settings.language')" :hint="t('settings.hint')">
            <div class="relative">
              <Globe
                class="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-zinc-400"
              />
              <select
                :class="[selectClass, 'pl-9']"
                :value="appStore.settings.language"
                :disabled="pendingField === 'language'"
                @change="onLanguageChange"
              >
                <option value="zh-CN">{{ t("settings.languageZh") }}</option>
                <option value="en">{{ t("settings.languageEn") }}</option>
              </select>
              <ChevronDown
                class="pointer-events-none absolute right-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-zinc-400"
              />
            </div>
          </FormField>
        </div>
      </SectionCard>

      <!-- 外观 -->
      <SectionCard
        :title="t('settings.appearance')"
        :description="t('settings.appearanceDesc')"
      >
        <FormField :label="t('settings.theme')">
          <div class="grid gap-2 sm:grid-cols-3">
            <button
              v-for="option in themeOptions"
              :key="option.value"
              type="button"
              class="flex flex-col items-center gap-1.5 rounded-xl border px-2 py-3 text-xs font-medium transition disabled:cursor-not-allowed disabled:opacity-60"
              :class="
                themeStore.mode === option.value
                  ? 'border-brand-400 bg-brand-50 text-brand-700 dark:border-brand-500/60 dark:bg-brand-500/10 dark:text-brand-300'
                  : 'border-zinc-200 text-zinc-600 hover:border-zinc-300 hover:bg-zinc-50 dark:border-zinc-700 dark:text-zinc-300 dark:hover:border-zinc-600 dark:hover:bg-zinc-800/50'
              "
              :disabled="pendingField === 'theme'"
              @click="chooseTheme(option.value)"
            >
              <component :is="option.icon" class="h-4 w-4" />
              {{ option.label }}
            </button>
          </div>
        </FormField>
      </SectionCard>

      <!-- 启动与自启 -->
      <SectionCard
        :title="t('settings.startup')"
        :description="t('settings.startupDesc')"
      >
        <div class="divide-y divide-zinc-200/70 dark:divide-zinc-800/70">
          <SettingSwitchRow
            :label="t('settings.launchAtLogin')"
            :description="t('settings.launchAtLoginDesc')"
            :model-value="appStore.settings.launchAtLogin"
            :pending="pendingField === 'launchAtLogin'"
            @update:model-value="onLaunchAtLogin"
          />
          <SettingSwitchRow
            :label="t('settings.startMinimized')"
            :description="t('settings.startMinimizedDesc')"
            :model-value="appStore.settings.startMinimized"
            :pending="pendingField === 'startMinimized'"
            @update:model-value="persist({ startMinimized: $event }, 'startMinimized')"
          />
          <SettingSwitchRow
            :label="t('settings.minimizeToTray')"
            :description="t('settings.minimizeToTrayDesc')"
            :model-value="appStore.settings.minimizeToTray"
            :pending="pendingField === 'minimizeToTray'"
            @update:model-value="persist({ minimizeToTray: $event }, 'minimizeToTray')"
          />
          <SettingSwitchRow
            :label="t('settings.runOnStart')"
            :description="t('settings.runOnStartDesc')"
            :model-value="appStore.settings.runWorkflowsOnStart"
            :pending="pendingField === 'runWorkflowsOnStart'"
            @update:model-value="
              persist({ runWorkflowsOnStart: $event }, 'runWorkflowsOnStart')
            "
          />
          <NumberSettingRow
            :label="t('settings.startupGrace')"
            :description="t('settings.startupGraceDesc')"
            :model-value="appStore.settings.startupGraceMs"
            :min="0"
            :max="600000"
            :step="100"
            suffix="ms"
            :disabled="pendingField === 'startupGraceMs'"
            @commit="persist({ startupGraceMs: $event }, 'startupGraceMs')"
          />
        </div>
      </SectionCard>

      <!-- 工作流 -->
      <SectionCard
        :title="t('settings.workflow')"
        :description="t('settings.workflowDesc')"
      >
        <div class="divide-y divide-zinc-200/70 dark:divide-zinc-800/70">
          <NumberSettingRow
            :label="t('settings.defaultNodeTimeout')"
            :model-value="appStore.settings.defaultNodeTimeoutMs"
            :min="0"
            :max="3600000"
            :step="1000"
            suffix="ms"
            :disabled="pendingField === 'defaultNodeTimeoutMs'"
            @commit="persist({ defaultNodeTimeoutMs: $event }, 'defaultNodeTimeoutMs')"
          />
          <NumberSettingRow
            :label="t('settings.maxConcurrency')"
            :model-value="appStore.settings.maxConcurrency"
            :min="1"
            :max="32"
            :disabled="pendingField === 'maxConcurrency'"
            @commit="persist({ maxConcurrency: $event }, 'maxConcurrency')"
          />
          <NumberSettingRow
            :label="t('settings.maxRunHistory')"
            :model-value="appStore.settings.maxRunHistory"
            :min="10"
            :max="500"
            :step="10"
            :disabled="pendingField === 'maxRunHistory'"
            @commit="persist({ maxRunHistory: $event }, 'maxRunHistory')"
          />
          <NumberSettingRow
            :label="t('settings.maxLogEntries')"
            :model-value="appStore.settings.maxLogEntries"
            :min="200"
            :max="20000"
            :step="100"
            :disabled="pendingField === 'maxLogEntries'"
            @commit="persist({ maxLogEntries: $event }, 'maxLogEntries')"
          />
          <SettingSwitchRow
            :label="t('settings.confirmTakeOver')"
            :model-value="appStore.settings.confirmTakeOver"
            :pending="pendingField === 'confirmTakeOver'"
            @update:model-value="
              persist({ confirmTakeOver: $event }, 'confirmTakeOver')
            "
          />
          <SettingSwitchRow
            :label="t('settings.confirmQuit')"
            :model-value="appStore.settings.confirmQuit"
            :pending="pendingField === 'confirmQuit'"
            @update:model-value="persist({ confirmQuit: $event }, 'confirmQuit')"
          />
        </div>
      </SectionCard>

      <!-- 平台与数据 -->
      <SectionCard
        class="lg:col-span-2"
        :title="t('settings.platform')"
        :description="t('settings.platformDesc')"
      >
        <div class="grid gap-4 lg:grid-cols-2">
          <div class="flex flex-col gap-2">
            <p class="text-[11px] font-semibold uppercase tracking-wider text-zinc-500 dark:text-zinc-400">
              {{ t("settings.configPath") }}
            </p>
            <div class="flex items-center gap-2">
              <code
                class="min-w-0 flex-1 truncate rounded-lg border border-zinc-200 bg-zinc-50 px-2.5 py-2 font-mono text-[11px] text-zinc-600 dark:border-zinc-800 dark:bg-zinc-950/50 dark:text-zinc-300"
                :title="appStore.systemInfo?.configPath"
              >
                {{ appStore.systemInfo?.configPath ?? "—" }}
              </code>
              <BaseButton
                variant="secondary"
                size="sm"
                :title="t('settings.openConfigDir')"
                @click="openDirectory('config')"
              >
                <FolderOpen class="h-3.5 w-3.5" />
              </BaseButton>
            </div>

            <p class="mt-1 text-[11px] font-semibold uppercase tracking-wider text-zinc-500 dark:text-zinc-400">
              {{ t("settings.logPath") }}
            </p>
            <div class="flex items-center gap-2">
              <code
                class="min-w-0 flex-1 truncate rounded-lg border border-zinc-200 bg-zinc-50 px-2.5 py-2 font-mono text-[11px] text-zinc-600 dark:border-zinc-800 dark:bg-zinc-950/50 dark:text-zinc-300"
                :title="appStore.systemInfo?.logPath"
              >
                {{ appStore.systemInfo?.logPath ?? "—" }}
              </code>
              <BaseButton
                variant="secondary"
                size="sm"
                :title="t('settings.openLogDir')"
                @click="openDirectory('log')"
              >
                <FolderOpen class="h-3.5 w-3.5" />
              </BaseButton>
            </div>
          </div>

          <div class="flex flex-col gap-2">
            <p class="text-[11px] font-semibold uppercase tracking-wider text-zinc-500 dark:text-zinc-400">
              {{ t("settings.configBackup") }}
            </p>
            <div class="flex flex-wrap gap-2">
              <BaseButton
                variant="secondary"
                size="sm"
                :loading="exportBusy"
                @click="exportConfig"
              >
                <Download v-if="!exportBusy" class="h-3.5 w-3.5" />
                {{ t("settings.exportConfig") }}
              </BaseButton>
              <BaseButton
                variant="secondary"
                size="sm"
                :loading="importBusy"
                @click="pickImport"
              >
                <Upload v-if="!importBusy" class="h-3.5 w-3.5" />
                {{ t("settings.importConfig") }}
              </BaseButton>
              <BaseButton
                variant="secondary"
                size="sm"
                :disabled="resetBusy"
                @click="resetOpen = true"
              >
                <RotateCcw class="h-3.5 w-3.5" />
                {{ t("settings.resetDefaults") }}
              </BaseButton>
            </div>

            <div
              class="mt-2 rounded-xl border border-red-200 bg-red-50/60 px-3 py-2.5 dark:border-red-500/25 dark:bg-red-500/5"
            >
              <p class="text-[11px] font-semibold uppercase tracking-wider text-red-600 dark:text-red-400">
                {{ t("common.dangerZone") }}
              </p>
              <BaseButton
                class="mt-2"
                variant="danger"
                size="sm"
                :loading="quitBusy"
                @click="quitOpen = true"
              >
                <Power v-if="!quitBusy" class="h-3.5 w-3.5" />
                {{ t("settings.quit") }}
              </BaseButton>
            </div>
          </div>
        </div>
      </SectionCard>

      <!-- 数据迁移 -->
      <SectionCard
        class="lg:col-span-2"
        :title="t('settings.migration')"
        :description="t('settings.migrationDesc')"
      >
        <div class="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
          <div class="min-w-0 max-w-2xl">
            <p class="text-xs leading-relaxed text-zinc-600 dark:text-zinc-300">
              {{ t("settings.importLegacyDesc") }}
            </p>
            <p
              class="mt-1 text-[11px] leading-relaxed text-zinc-500 dark:text-zinc-400"
            >
              {{ t("settings.legacyRepeatHint") }}
            </p>
            <p
              v-if="legacyPath"
              class="mt-1 truncate font-mono text-[11px] text-zinc-400 dark:text-zinc-500"
              :title="legacyPath"
            >
              {{ t("settings.legacyFile") }}：{{ legacyPath }}
            </p>
          </div>
          <BaseButton
            variant="primary"
            size="sm"
            :loading="legacyBusy"
            :disabled="legacyBusy"
            @click="importLegacy"
          >
            <History v-if="!legacyBusy" class="h-3.5 w-3.5" />
            {{ t("settings.importLegacy") }}
          </BaseButton>
        </div>

        <div
          v-if="legacyResult"
          class="mt-3 rounded-xl border border-zinc-200 bg-zinc-50/60 px-3 py-2.5 dark:border-zinc-800 dark:bg-zinc-900/40"
        >
          <div class="flex flex-wrap items-center gap-x-2 gap-y-1 text-xs">
            <BaseBadge :variant="legacyResult.replaced ? 'warn' : 'success'">
              {{
                legacyResult.replaced
                  ? t("settings.legacyReplaced")
                  : t("settings.legacyCreated")
              }}
            </BaseBadge>
            <span class="font-medium text-zinc-800 dark:text-zinc-100">
              {{ legacyResult.workflowName }}
            </span>
            <span class="text-zinc-500 dark:text-zinc-400">
              {{ t("workflow.nodeCount", { count: legacyResult.nodeCount }) }}
              ·
              {{ t("workflow.edgeCount", { count: legacyResult.edgeCount }) }}
            </span>
          </div>
          <ul
            v-if="legacyResult.warnings.length"
            class="mt-2 list-disc space-y-0.5 pl-5 text-[11px] leading-relaxed text-amber-700 dark:text-amber-300"
          >
            <li v-for="warning in legacyResult.warnings" :key="warning">
              {{ warning }}
            </li>
          </ul>
        </div>
      </SectionCard>

      <!-- 关于 -->
      <SectionCard
        class="lg:col-span-2"
        :title="t('settings.about')"
      >
        <div class="grid gap-4 sm:grid-cols-2">
          <div class="flex flex-col gap-2">
            <div class="flex items-center gap-2">
              <span
                class="flex h-9 w-9 items-center justify-center rounded-xl bg-brand-50 text-brand-600 dark:bg-brand-500/15 dark:text-brand-300"
              >
                <Workflow class="h-5 w-5" />
              </span>
              <div class="min-w-0">
                <p class="text-sm font-semibold text-zinc-800 dark:text-zinc-100">
                  DaemonMan
                </p>
                <p class="text-[11px] text-zinc-500 dark:text-zinc-400">
                  {{ t("settings.appVersion") }} v{{ version }}
                </p>
              </div>
            </div>
            <div class="text-[11px] leading-relaxed text-zinc-500 dark:text-zinc-400">
              <p class="font-semibold uppercase tracking-wider">
                {{ t("settings.builtWith") }}
              </p>
              <p class="mt-1 font-mono">{{ stack }}</p>
            </div>
          </div>

          <div
            class="flex items-start gap-2 rounded-xl border px-3 py-2.5 text-[11px] leading-relaxed"
            :class="
              elevated
                ? 'border-emerald-200 bg-emerald-50/60 text-emerald-800 dark:border-emerald-500/30 dark:bg-emerald-500/10 dark:text-emerald-200'
                : 'border-amber-200 bg-amber-50/60 text-amber-900 dark:border-amber-500/30 dark:bg-amber-500/10 dark:text-amber-200'
            "
          >
            <ShieldCheck v-if="elevated" class="mt-0.5 h-4 w-4 shrink-0" />
            <ShieldX v-else class="mt-0.5 h-4 w-4 shrink-0" />
            <span>
              {{
                elevated
                  ? t("settings.elevatedHint")
                  : t("settings.noElevatedHint")
              }}
            </span>
          </div>
        </div>
      </SectionCard>
    </div>

    <!-- 导入确认 -->
    <BaseModal
      :open="importOpen"
      :title="t('settings.importConfig')"
      size="sm"
      :closable="!importBusy"
      @close="importOpen = false"
    >
      <p class="text-xs leading-relaxed text-zinc-600 dark:text-zinc-300">
        {{ t("settings.importConfirm") }}
      </p>
      <p
        v-if="pendingImportPath"
        class="mt-2 truncate rounded-lg bg-zinc-100 px-2.5 py-1.5 font-mono text-[11px] text-zinc-500 dark:bg-zinc-950/60 dark:text-zinc-400"
        :title="pendingImportPath"
      >
        {{ pendingImportPath }}
      </p>
      <template #footer>
        <BaseButton
          variant="secondary"
          size="sm"
          :disabled="importBusy"
          @click="importOpen = false"
        >
          {{ t("common.cancel") }}
        </BaseButton>
        <BaseButton
          variant="primary"
          size="sm"
          :loading="importBusy"
          @click="confirmImport"
        >
          {{ t("common.confirm") }}
        </BaseButton>
      </template>
    </BaseModal>

    <!-- 恢复默认确认 -->
    <BaseModal
      :open="resetOpen"
      :title="t('settings.resetDefaults')"
      size="sm"
      :closable="!resetBusy"
      @close="resetOpen = false"
    >
      <p class="text-xs leading-relaxed text-zinc-600 dark:text-zinc-300">
        {{ t("settings.resetConfirm") }}
      </p>
      <template #footer>
        <BaseButton
          variant="secondary"
          size="sm"
          :disabled="resetBusy"
          @click="resetOpen = false"
        >
          {{ t("common.cancel") }}
        </BaseButton>
        <BaseButton
          variant="danger"
          size="sm"
          :loading="resetBusy"
          @click="confirmReset"
        >
          {{ t("common.confirm") }}
        </BaseButton>
      </template>
    </BaseModal>

    <!-- 退出确认 -->
    <BaseModal
      :open="quitOpen"
      :title="t('settings.quit')"
      size="sm"
      :closable="!quitBusy"
      @close="quitOpen = false"
    >
      <p class="text-xs leading-relaxed text-zinc-600 dark:text-zinc-300">
        {{ t("settings.quitConfirm") }}
      </p>
      <template #footer>
        <BaseButton
          variant="secondary"
          size="sm"
          :disabled="quitBusy"
          @click="quitOpen = false"
        >
          {{ t("common.cancel") }}
        </BaseButton>
        <BaseButton
          variant="danger"
          size="sm"
          :loading="quitBusy"
          @click="confirmQuit"
        >
          {{ t("settings.quit") }}
        </BaseButton>
      </template>
    </BaseModal>
  </div>
</template>
