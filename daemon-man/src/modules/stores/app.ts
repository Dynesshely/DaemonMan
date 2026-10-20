import { defineStore } from "pinia";
import { computed, ref } from "vue";

import { api } from "@/api";
import type { AppConfig, AppSettings, SystemInfo } from "@/api";

import { useThemeStore, type ThemeMode } from "./theme";
import { useUiStore } from "./ui";

function normalizeTheme(value: string): ThemeMode {
  return value === "light" || value === "dark" ? value : "system";
}

export const useAppStore = defineStore("app", () => {
  const config = ref<AppConfig | null>(null);
  const systemInfo = ref<SystemInfo | null>(null);
  const loading = ref(false);
  const initialized = ref(false);

  const settings = computed<AppSettings | null>(
    () => config.value?.settings ?? null,
  );

  async function init() {
    if (initialized.value) return;
    loading.value = true;
    try {
      const loaded = await api.getConfig();
      config.value = loaded;
      useThemeStore().setMode(normalizeTheme(loaded.settings.theme));
      useUiStore().setLocale(
        loaded.settings.language === "en" ? "en" : "zh-CN",
      );
      await refreshSystemInfo();
      initialized.value = true;
    } finally {
      loading.value = false;
    }
  }

  async function refreshConfig() {
    config.value = await api.getConfig();
  }

  async function refreshSystemInfo() {
    try {
      systemInfo.value = await api.getSystemInfo();
    } catch (error) {
      console.warn("获取系统信息失败", error);
    }
  }

  async function saveSettings(patch: Partial<AppSettings>) {
    if (!config.value) return;
    const next: AppSettings = { ...config.value.settings, ...patch };
    config.value = await api.saveSettings(next);
    useThemeStore().setMode(normalizeTheme(config.value.settings.theme));
    useUiStore().setLocale(
      config.value.settings.language === "en" ? "en" : "zh-CN",
    );
    await refreshSystemInfo();
  }

  async function setLaunchAtLogin(enabled: boolean) {
    const actual = await api.setSelfAutostart(enabled);
    if (config.value) {
      config.value.settings.launchAtLogin = actual;
    }
  }

  async function exportConfig(path: string) {
    await api.exportConfig(path);
  }

  async function importConfig(path: string) {
    config.value = await api.importConfig(path);
    useThemeStore().setMode(normalizeTheme(config.value.settings.theme));
    useUiStore().setLocale(
      config.value.settings.language === "en" ? "en" : "zh-CN",
    );
    await refreshSystemInfo();
  }

  return {
    config,
    settings,
    systemInfo,
    loading,
    initialized,
    init,
    refreshConfig,
    refreshSystemInfo,
    saveSettings,
    setLaunchAtLogin,
    exportConfig,
    importConfig,
  };
});
