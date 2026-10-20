<script setup lang="ts">
import { computed } from "vue";
import { useRoute } from "vue-router";
import {
  Activity,
  CircleDot,
  GitBranch,
  LayoutDashboard,
  Rocket,
  ScrollText,
  Settings,
  ShieldCheck,
  ChevronLeft,
  ChevronRight,
} from "lucide-vue-next";

import { useI18n } from "@/modules/locales";
import { useAppStore } from "@/modules/stores/app";
import { useUiStore } from "@/modules/stores/ui";

const route = useRoute();
const ui = useUiStore();
const appStore = useAppStore();
const { t } = useI18n();

const manageItems = [
  { to: "/", key: "nav.overview", icon: LayoutDashboard },
  { to: "/workflows", key: "nav.workflows", icon: GitBranch },
  { to: "/startup", key: "nav.startup", icon: Rocket },
  { to: "/services", key: "nav.services", icon: Activity },
];

const systemItems = [
  { to: "/logs", key: "nav.logs", icon: ScrollText },
  { to: "/settings", key: "nav.settings", icon: Settings },
];

const collapsed = computed(() => ui.sidebarCollapsed);

function isActive(path: string) {
  if (path === "/") return route.path === "/";
  return route.path.startsWith(path);
}

const version = computed(() => appStore.systemInfo?.appVersion ?? "0.1.0");
const elevated = computed(() => appStore.systemInfo?.elevated ?? false);
</script>

<template>
  <aside
    class="flex h-full shrink-0 flex-col border-r border-zinc-200 bg-white/80 backdrop-blur transition-[width] duration-200 dark:border-zinc-800 dark:bg-zinc-900/60"
    :class="collapsed ? 'w-[68px]' : 'w-56'"
  >
    <!-- 品牌 -->
    <div class="flex h-14 items-center gap-2.5 px-4">
      <div
        class="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-gradient-to-br from-brand-500 to-brand-700 text-white shadow-sm shadow-brand-600/30"
      >
        <CircleDot class="h-4.5 w-4.5" />
      </div>
      <div v-if="!collapsed" class="min-w-0">
        <p class="truncate text-sm font-semibold tracking-tight">DaemonMan</p>
        <p class="truncate text-[10px] text-zinc-400 dark:text-zinc-500">
          startup pipeline
        </p>
      </div>
    </div>

    <nav class="min-h-0 flex-1 space-y-5 overflow-y-auto px-2.5 py-3">
      <div>
        <p
          v-if="!collapsed"
          class="mb-1.5 px-2 text-[10px] font-semibold uppercase tracking-wider text-zinc-400 dark:text-zinc-500"
        >
          {{ t("nav.group.manage") }}
        </p>
        <div class="space-y-0.5">
          <RouterLink
            v-for="item in manageItems"
            :key="item.to"
            :to="item.to"
            class="group flex items-center gap-2.5 rounded-lg px-2.5 py-2 text-sm font-medium transition"
            :class="
              isActive(item.to)
                ? 'bg-brand-50 text-brand-700 dark:bg-brand-500/15 dark:text-brand-300'
                : 'text-zinc-600 hover:bg-zinc-100 dark:text-zinc-300 dark:hover:bg-zinc-800/70'
            "
            :title="collapsed ? t(item.key) : undefined"
          >
            <component
              :is="item.icon"
              class="h-4 w-4 shrink-0"
              :class="isActive(item.to) ? 'text-brand-600 dark:text-brand-400' : ''"
            />
            <span v-if="!collapsed" class="truncate">{{ t(item.key) }}</span>
            <span
              v-if="!collapsed && isActive(item.to)"
              class="ml-auto h-1.5 w-1.5 rounded-full bg-brand-500"
            />
          </RouterLink>
        </div>
      </div>

      <div>
        <p
          v-if="!collapsed"
          class="mb-1.5 px-2 text-[10px] font-semibold uppercase tracking-wider text-zinc-400 dark:text-zinc-500"
        >
          {{ t("nav.group.system") }}
        </p>
        <div class="space-y-0.5">
          <RouterLink
            v-for="item in systemItems"
            :key="item.to"
            :to="item.to"
            class="group flex items-center gap-2.5 rounded-lg px-2.5 py-2 text-sm font-medium transition"
            :class="
              isActive(item.to)
                ? 'bg-brand-50 text-brand-700 dark:bg-brand-500/15 dark:text-brand-300'
                : 'text-zinc-600 hover:bg-zinc-100 dark:text-zinc-300 dark:hover:bg-zinc-800/70'
            "
            :title="collapsed ? t(item.key) : undefined"
          >
            <component
              :is="item.icon"
              class="h-4 w-4 shrink-0"
              :class="isActive(item.to) ? 'text-brand-600 dark:text-brand-400' : ''"
            />
            <span v-if="!collapsed" class="truncate">{{ t(item.key) }}</span>
          </RouterLink>
        </div>
      </div>
    </nav>

    <div class="border-t border-zinc-200 px-2.5 py-2.5 dark:border-zinc-800">
      <div
        v-if="!collapsed"
        class="mb-2 flex items-center justify-between rounded-lg bg-zinc-50 px-2.5 py-1.5 text-[11px] text-zinc-500 dark:bg-zinc-800/50 dark:text-zinc-400"
      >
        <span class="inline-flex items-center gap-1.5">
          <ShieldCheck
            class="h-3.5 w-3.5"
            :class="elevated ? 'text-emerald-500' : 'text-zinc-400'"
          />
          {{ elevated ? t("overview.elevated") : t("overview.notElevated") }}
        </span>
        <span class="tabular">v{{ version }}</span>
      </div>
      <button
        type="button"
        class="flex w-full items-center justify-center gap-2 rounded-lg px-2 py-1.5 text-xs text-zinc-500 transition hover:bg-zinc-100 hover:text-zinc-700 dark:text-zinc-400 dark:hover:bg-zinc-800 dark:hover:text-zinc-200"
        :title="t(collapsed ? 'nav.expand' : 'nav.collapse')"
        @click="ui.toggleSidebar()"
      >
        <ChevronRight v-if="collapsed" class="h-4 w-4" />
        <ChevronLeft v-else class="h-4 w-4" />
        <span v-if="!collapsed">{{ t("nav.collapse") }}</span>
      </button>
    </div>
  </aside>
</template>
