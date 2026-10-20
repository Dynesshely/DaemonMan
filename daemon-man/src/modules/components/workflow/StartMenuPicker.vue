<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from "vue";
import { AppWindow, RefreshCw, Search } from "lucide-vue-next";

import { api } from "@/api";
import type { StartMenuApp } from "@/api";
import BaseBadge from "@/modules/components/ui/BaseBadge.vue";
import BaseButton from "@/modules/components/ui/BaseButton.vue";
import BaseModal from "@/modules/components/ui/BaseModal.vue";
import BaseSpinner from "@/modules/components/ui/BaseSpinner.vue";
import EmptyState from "@/modules/components/ui/EmptyState.vue";
import { inputClass } from "@/modules/components/ui/form";
import { useI18n } from "@/modules/locales";
import { useIconStore } from "@/modules/stores/icons";

import { errorText } from "./shared";

const props = defineProps<{ open: boolean }>();

const emit = defineEmits<{
  close: [];
  pick: [app: StartMenuApp];
  manual: [];
}>();

const { t } = useI18n();
const icons = useIconStore();

const apps = ref<StartMenuApp[]>([]);
const loading = ref(false);
const error = ref<string | null>(null);
const search = ref("");
const activeIndex = ref(0);
const loaded = ref(false);

const searchRef = ref<HTMLInputElement | null>(null);
const listRef = ref<HTMLElement | null>(null);

const filtered = computed(() => {
  const keyword = search.value.trim().toLowerCase();
  if (!keyword) return apps.value;
  return apps.value.filter((app) =>
    `${app.name} ${app.target ?? ""} ${app.path}`.toLowerCase().includes(keyword),
  );
});

function iconOf(app: StartMenuApp): string | null {
  return icons.get(app.target) ?? icons.get(app.path);
}

function iconSrc(app: StartMenuApp): string {
  return iconOf(app) ?? "";
}

function locationLabel(app: StartMenuApp): string {
  return app.location === "machine"
    ? t("workflow.pickAppMachine")
    : t("workflow.pickAppUser");
}

async function load() {
  loading.value = true;
  error.value = null;
  try {
    const list = await api.listStartMenuApps();
    apps.value = [...list].sort((left, right) =>
      left.name.localeCompare(right.name),
    );
    loaded.value = true;
    activeIndex.value = 0;
    // 优先用解析出的 target（exe）取图标，其次才是 .lnk 本身。
    const paths: string[] = [];
    for (const app of apps.value) {
      if (app.target) paths.push(app.target);
      if (app.path) paths.push(app.path);
    }
    void icons.ensure(paths);
  } catch (err) {
    error.value = errorText(err);
    loaded.value = false;
  } finally {
    loading.value = false;
  }
}

function pick(app: StartMenuApp) {
  emit("pick", app);
}

function move(delta: number) {
  const total = filtered.value.length;
  if (total === 0) return;
  activeIndex.value = (activeIndex.value + delta + total) % total;
}

function onKeydown(event: KeyboardEvent) {
  if (!props.open) return;
  if (event.key === "ArrowDown") {
    event.preventDefault();
    move(1);
  } else if (event.key === "ArrowUp") {
    event.preventDefault();
    move(-1);
  } else if (event.key === "Enter") {
    const app = filtered.value[activeIndex.value];
    if (app) {
      event.preventDefault();
      pick(app);
    }
  }
}

watch(
  () => props.open,
  (open) => {
    if (open) {
      search.value = "";
      activeIndex.value = 0;
      window.addEventListener("keydown", onKeydown);
      if (!loaded.value) void load();
      void nextTick(() => searchRef.value?.focus());
    } else {
      window.removeEventListener("keydown", onKeydown);
    }
  },
  { immediate: true },
);

watch(filtered, (list) => {
  if (activeIndex.value >= list.length) activeIndex.value = 0;
});

// 键盘移动时让选中行保持可见。
watch(activeIndex, (index) => {
  const row = listRef.value?.querySelector<HTMLElement>(
    `[data-index="${index}"]`,
  );
  row?.scrollIntoView({ block: "nearest" });
});

onBeforeUnmount(() => window.removeEventListener("keydown", onKeydown));
</script>

<template>
  <BaseModal
    :open="props.open"
    :title="t('workflow.pickAppTitle')"
    :description="t('workflow.pickAppHint')"
    size="lg"
    @close="emit('close')"
  >
    <div class="flex h-[420px] flex-col gap-2">
      <div class="flex shrink-0 items-center gap-2">
        <div class="relative min-w-0 flex-1">
          <Search
            class="pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-zinc-400 dark:text-zinc-500"
          />
          <input
            ref="searchRef"
            v-model="search"
            type="search"
            :placeholder="t('workflow.pickAppSearch')"
            :class="inputClass"
            class="py-1.5! pl-8! text-xs!"
          />
        </div>
        <BaseBadge variant="neutral">
          {{ t("workflow.pickAppCount", { count: filtered.length }) }}
        </BaseBadge>
      </div>

      <div
        ref="listRef"
        class="min-h-0 flex-1 overflow-y-auto rounded-lg border border-zinc-200 dark:border-zinc-800"
      >
        <div
          v-if="loading"
          class="flex h-full items-center justify-center gap-2 text-xs text-zinc-500 dark:text-zinc-400"
        >
          <BaseSpinner size="sm" />
          {{ t("workflow.pickAppLoading") }}
        </div>

        <div
          v-else-if="error"
          class="flex h-full flex-col items-center justify-center gap-2 p-4 text-center"
        >
          <p class="max-w-sm text-xs leading-relaxed text-red-600 dark:text-red-400">
            {{ t("workflow.pickAppFailed", { error }) }}
          </p>
          <BaseButton variant="secondary" size="sm" @click="load()">
            <RefreshCw class="h-3.5 w-3.5" />
            {{ t("common.retry") }}
          </BaseButton>
        </div>

        <EmptyState
          v-else-if="filtered.length === 0"
          :title="t('workflow.pickAppEmpty')"
          compact
        >
          <template #icon>
            <AppWindow class="h-5 w-5" />
          </template>
        </EmptyState>

        <ul v-else>
          <li v-for="(app, index) in filtered" :key="`${app.location}:${app.path}`">
            <button
              type="button"
              :data-index="index"
              class="flex w-full items-center gap-2.5 border-b border-zinc-100 px-2.5 py-1.5 text-left transition last:border-b-0 dark:border-zinc-800/60"
              :class="
                index === activeIndex
                  ? 'bg-brand-50 dark:bg-brand-500/10'
                  : 'hover:bg-zinc-50 dark:hover:bg-zinc-800/50'
              "
              @click="activeIndex = index"
              @dblclick="pick(app)"
            >
              <span
                class="flex h-6 w-6 shrink-0 items-center justify-center rounded"
              >
                <img
                  v-if="iconOf(app)"
                  :src="iconSrc(app)"
                  alt=""
                  class="h-5 w-5 rounded-sm object-contain"
                />
                <AppWindow v-else class="h-4 w-4 text-zinc-400 dark:text-zinc-500" />
              </span>
              <span class="min-w-0 flex-1">
                <span class="flex items-center gap-1.5">
                  <span
                    class="truncate text-xs font-medium text-zinc-700 dark:text-zinc-200"
                  >
                    {{ app.name }}
                  </span>
                  <span
                    class="shrink-0 rounded-full bg-zinc-100 px-1.5 text-[10px] text-zinc-500 dark:bg-zinc-800 dark:text-zinc-400"
                  >
                    {{ locationLabel(app) }}
                  </span>
                </span>
                <span
                  class="block truncate font-mono text-[10px] text-zinc-400 dark:text-zinc-500"
                  :title="app.target ?? app.path"
                >
                  {{ app.target ?? app.path }}
                </span>
              </span>
            </button>
          </li>
        </ul>
      </div>
    </div>

    <template #footer>
      <BaseButton variant="ghost" size="sm" @click="emit('manual')">
        {{ t("workflow.pickAppManual") }}
      </BaseButton>
      <BaseButton variant="secondary" size="sm" @click="emit('close')">
        {{ t("common.cancel") }}
      </BaseButton>
    </template>
  </BaseModal>
</template>
