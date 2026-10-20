<script setup lang="ts">
import { computed, ref } from "vue";
import { Copy, GitBranch, Plus, Rocket, Search, Trash2 } from "lucide-vue-next";

import type { Workflow } from "@/api";
import BaseBadge from "@/modules/components/ui/BaseBadge.vue";
import BaseButton from "@/modules/components/ui/BaseButton.vue";
import BaseModal from "@/modules/components/ui/BaseModal.vue";
import BaseSpinner from "@/modules/components/ui/BaseSpinner.vue";
import EmptyState from "@/modules/components/ui/EmptyState.vue";
import { inputClass } from "@/modules/components/ui/form";
import { useI18n } from "@/modules/locales";
import { useWorkflowStore } from "@/modules/stores/workflows";
import { runStatusVariant } from "@/modules/utils/status";

import { panelClass, panelHeaderClass } from "./shared";

const props = defineProps<{
  workflows: Workflow[];
  selectedId: string | null;
  loading: boolean;
}>();

const emit = defineEmits<{
  select: [id: string];
  create: [];
  duplicate: [id: string];
  remove: [id: string];
}>();

const store = useWorkflowStore();
const { t } = useI18n();

const search = ref("");
const pendingDelete = ref<Workflow | null>(null);

const filtered = computed(() => {
  const keyword = search.value.trim().toLowerCase();
  if (!keyword) return props.workflows;
  return props.workflows.filter((workflow) =>
    `${workflow.name} ${workflow.description}`
      .toLowerCase()
      .includes(keyword),
  );
});

function latestRun(workflowId: string) {
  return store.latestRunFor(workflowId);
}

function confirmDelete() {
  const target = pendingDelete.value;
  if (!target) return;
  pendingDelete.value = null;
  emit("remove", target.id);
}
</script>

<template>
  <aside :class="panelClass">
    <header :class="panelHeaderClass">
      <div class="min-w-0">
        <p class="truncate text-xs font-semibold text-zinc-700 dark:text-zinc-200">
          {{ t("workflow.list") }}
        </p>
        <p class="text-[10px] text-zinc-400 dark:text-zinc-500">
          {{ t("common.count") }} {{ workflows.length }}
        </p>
      </div>
      <BaseButton
        variant="subtle"
        size="sm"
        :title="t('workflow.new')"
        @click="emit('create')"
      >
        <Plus class="h-3.5 w-3.5" />
        {{ t("workflow.new") }}
      </BaseButton>
    </header>

    <div class="shrink-0 px-3 pb-2 pt-2.5">
      <div class="relative">
        <Search
          class="pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-zinc-400 dark:text-zinc-500"
        />
        <input
          v-model="search"
          type="search"
          :placeholder="t('workflow.searchPlaceholder')"
          :class="inputClass"
          class="py-1.5! pl-8! text-xs!"
        />
      </div>
    </div>

    <div class="min-h-0 flex-1 overflow-y-auto px-2 pb-2">
      <div v-if="loading" class="flex items-center justify-center py-12">
        <BaseSpinner />
      </div>

      <EmptyState
        v-else-if="workflows.length === 0"
        :title="t('workflow.empty')"
        :description="t('workflow.emptyHint')"
        compact
      >
        <template #icon>
          <GitBranch class="h-5 w-5" />
        </template>
        <template #action>
          <BaseButton variant="primary" size="sm" @click="emit('create')">
            <Plus class="h-3.5 w-3.5" />
            {{ t("workflow.new") }}
          </BaseButton>
        </template>
      </EmptyState>

      <EmptyState
        v-else-if="filtered.length === 0"
        :title="t('common.empty')"
        :description="t('common.search')"
        compact
      />

      <ul v-else class="space-y-1">
        <li v-for="workflow in filtered" :key="workflow.id">
          <div
            role="button"
            tabindex="0"
            class="group relative cursor-pointer rounded-lg border px-2.5 py-2 transition"
            :class="
              workflow.id === selectedId
                ? 'border-brand-400/70 bg-brand-50/70 dark:border-brand-500/50 dark:bg-brand-500/10'
                : 'border-transparent hover:border-zinc-200 hover:bg-zinc-50 dark:hover:border-zinc-800 dark:hover:bg-zinc-800/50'
            "
            @click="emit('select', workflow.id)"
            @keydown.enter.prevent="emit('select', workflow.id)"
          >
            <div class="flex items-start gap-1.5">
              <p
                class="min-w-0 flex-1 truncate text-sm font-medium"
                :class="
                  workflow.id === selectedId
                    ? 'text-brand-700 dark:text-brand-200'
                    : 'text-zinc-700 dark:text-zinc-200'
                "
              >
                {{ workflow.name || t("common.unknown") }}
              </p>

              <div
                class="flex shrink-0 items-center gap-0.5 opacity-0 transition group-hover:opacity-100 group-focus-within:opacity-100"
              >
                <button
                  type="button"
                  class="rounded p-1 text-zinc-400 transition hover:bg-zinc-200/70 hover:text-zinc-600 dark:hover:bg-zinc-700/70 dark:hover:text-zinc-200"
                  :title="t('common.duplicate')"
                  @click.stop="emit('duplicate', workflow.id)"
                >
                  <Copy class="h-3.5 w-3.5" />
                </button>
                <button
                  type="button"
                  class="rounded p-1 text-zinc-400 transition hover:bg-red-100 hover:text-red-600 dark:hover:bg-red-500/15 dark:hover:text-red-400"
                  :title="t('common.delete')"
                  @click.stop="pendingDelete = workflow"
                >
                  <Trash2 class="h-3.5 w-3.5" />
                </button>
              </div>

              <BaseBadge
                v-if="latestRun(workflow.id)"
                :variant="runStatusVariant(latestRun(workflow.id)!.status)"
                dot
                :pulse="latestRun(workflow.id)!.status === 'running'"
              >
                {{ t(`status.${latestRun(workflow.id)!.status}`) }}
              </BaseBadge>
            </div>

            <div
              class="mt-1 flex flex-wrap items-center gap-x-2 gap-y-0.5 text-[11px] text-zinc-400 dark:text-zinc-500"
            >
              <span class="inline-flex items-center gap-1">
                <GitBranch class="h-3 w-3" />
                {{ t("workflow.nodeCount", { count: workflow.nodes.length }) }}
              </span>
              <span
                v-if="workflow.enabled"
                class="inline-flex items-center gap-1 text-brand-600 dark:text-brand-400"
              >
                <Rocket class="h-3 w-3" />
                {{ t("workflow.enabledOnStartup") }}
              </span>
              <span v-if="workflow.description" class="min-w-0 flex-1 truncate">
                {{ workflow.description }}
              </span>
            </div>
          </div>
        </li>
      </ul>
    </div>

    <BaseModal
      :open="pendingDelete !== null"
      :title="t('common.confirmDeleteTitle')"
      :description="
        pendingDelete
          ? t('workflow.deleteConfirm', { name: pendingDelete.name })
          : undefined
      "
      size="sm"
      @close="pendingDelete = null"
    >
      <template #footer>
        <BaseButton variant="secondary" size="sm" @click="pendingDelete = null">
          {{ t("common.cancel") }}
        </BaseButton>
        <BaseButton variant="danger" size="sm" @click="confirmDelete">
          {{ t("common.delete") }}
        </BaseButton>
      </template>
    </BaseModal>
  </aside>
</template>
