<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { AlertTriangle, ChevronDown, Layers, PlusCircle } from "lucide-vue-next";

import type { TakeOverResult, Workflow } from "@/api";
import BaseButton from "@/modules/components/ui/BaseButton.vue";
import BaseModal from "@/modules/components/ui/BaseModal.vue";
import FormField from "@/modules/components/ui/FormField.vue";
import { inputClass, selectClass } from "@/modules/components/ui/form";
import { useI18n } from "@/modules/locales";

const props = defineProps<{
  open: boolean;
  count: number;
  workflows: Workflow[];
  busy?: boolean;
  result: TakeOverResult | null;
}>();

const emit = defineEmits<{
  close: [];
  confirm: [payload: { workflowId?: string; workflowName?: string }];
}>();

const { t } = useI18n();

const mode = ref<"new" | "existing">("new");
const newName = ref(t("startup.takeOverDefaultName"));
const targetId = ref("");

watch(
  () => props.open,
  (open) => {
    if (!open) return;
    mode.value = "new";
    newName.value = t("startup.takeOverDefaultName");
    targetId.value = props.workflows[0]?.id ?? "";
  },
);

const canConfirm = computed(() => {
  if (props.busy) return false;
  if (mode.value === "existing") return Boolean(targetId.value);
  return newName.value.trim().length > 0;
});

function confirm() {
  if (!canConfirm.value) return;
  if (mode.value === "existing") {
    emit("confirm", { workflowId: targetId.value });
  } else {
    emit("confirm", { workflowName: newName.value.trim() });
  }
}
</script>

<template>
  <BaseModal
    :open="open"
    :title="t('startup.takeOverTitle', { count })"
    :description="t('startup.takeOverDesc')"
    size="md"
    :closable="!busy"
    @close="emit('close')"
  >
    <div class="flex flex-col gap-4">
      <div
        class="rounded-xl border border-brand-200 bg-brand-50/60 px-3.5 py-3 text-xs leading-relaxed text-brand-800 dark:border-brand-500/30 dark:bg-brand-500/10 dark:text-brand-200"
      >
        {{ t("startup.selected", { count }) }}
      </div>

      <div v-if="result" class="flex flex-col gap-2">
        <div
          class="flex items-start gap-2 rounded-xl border border-emerald-200 bg-emerald-50 px-3 py-2.5 text-xs text-emerald-800 dark:border-emerald-500/30 dark:bg-emerald-500/10 dark:text-emerald-200"
        >
          <Layers class="mt-0.5 h-3.5 w-3.5 shrink-0" />
          <span>
            {{ t("startup.takeOverSuccess", { count: result.items.length }) }}
            · {{ result.workflow.name }}
          </span>
        </div>

        <div
          v-if="result.skipped.length"
          class="rounded-xl border border-amber-200 bg-amber-50 px-3 py-2.5 text-xs text-amber-900 dark:border-amber-500/30 dark:bg-amber-500/10 dark:text-amber-200"
        >
          <p class="flex items-center gap-1.5 font-medium">
            <AlertTriangle class="h-3.5 w-3.5 shrink-0" />
            {{ t("startup.takeOverPartial", { count: result.skipped.length }) }}
          </p>
          <ul class="mt-1 list-disc space-y-0.5 pl-5">
            <li v-for="item in result.skipped" :key="item">{{ item }}</li>
          </ul>
        </div>

        <div
          v-if="result.failures.length"
          class="rounded-xl border border-red-200 bg-red-50 px-3 py-2.5 text-xs text-red-800 dark:border-red-500/30 dark:bg-red-500/10 dark:text-red-200"
        >
          <p class="flex items-center gap-1.5 font-medium">
            <AlertTriangle class="h-3.5 w-3.5 shrink-0" />
            {{ t("startup.takeOverPartial", { count: result.failures.length }) }}
          </p>
          <ul class="mt-1 list-disc space-y-0.5 pl-5">
            <li v-for="item in result.failures" :key="item">{{ item }}</li>
          </ul>
        </div>
      </div>

      <template v-else>
        <div class="grid gap-2 sm:grid-cols-2">
          <button
            type="button"
            class="flex items-start gap-2.5 rounded-xl border px-3 py-2.5 text-left transition"
            :class="
              mode === 'new'
                ? 'border-brand-400 bg-brand-50 dark:border-brand-500/60 dark:bg-brand-500/10'
                : 'border-zinc-200 hover:border-zinc-300 dark:border-zinc-700 dark:hover:border-zinc-600'
            "
            @click="mode = 'new'"
          >
            <PlusCircle
              class="mt-0.5 h-4 w-4 shrink-0"
              :class="
                mode === 'new'
                  ? 'text-brand-600 dark:text-brand-300'
                  : 'text-zinc-400'
              "
            />
            <span class="min-w-0">
              <span
                class="block text-xs font-medium text-zinc-800 dark:text-zinc-100"
              >
                {{ t("startup.newWorkflow") }}
              </span>
            </span>
          </button>

          <button
            type="button"
            class="flex items-start gap-2.5 rounded-xl border px-3 py-2.5 text-left transition disabled:cursor-not-allowed disabled:opacity-50"
            :class="
              mode === 'existing'
                ? 'border-brand-400 bg-brand-50 dark:border-brand-500/60 dark:bg-brand-500/10'
                : 'border-zinc-200 hover:border-zinc-300 dark:border-zinc-700 dark:hover:border-zinc-600'
            "
            :disabled="workflows.length === 0"
            @click="mode = 'existing'"
          >
            <Layers
              class="mt-0.5 h-4 w-4 shrink-0"
              :class="
                mode === 'existing'
                  ? 'text-brand-600 dark:text-brand-300'
                  : 'text-zinc-400'
              "
            />
            <span class="min-w-0">
              <span
                class="block text-xs font-medium text-zinc-800 dark:text-zinc-100"
              >
                {{ t("startup.appendWorkflow") }}
              </span>
              <span
                class="mt-0.5 block truncate text-[11px] text-zinc-500 dark:text-zinc-400"
              >
                {{ t("startup.targetWorkflow") }}
              </span>
            </span>
          </button>
        </div>

        <FormField v-if="mode === 'new'" :label="t('startup.workflowName')">
          <input v-model="newName" :class="inputClass" type="text" />
        </FormField>

        <FormField v-else :label="t('startup.targetWorkflow')">
          <div class="relative">
            <select v-model="targetId" :class="selectClass">
              <option
                v-for="workflow in workflows"
                :key="workflow.id"
                :value="workflow.id"
              >
                {{ workflow.name }}
              </option>
            </select>
            <ChevronDown
              class="pointer-events-none absolute right-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-zinc-400"
            />
          </div>
        </FormField>
      </template>
    </div>

    <template #footer>
      <BaseButton variant="secondary" size="sm" @click="emit('close')">
        {{ t("common.close") }}
      </BaseButton>
      <BaseButton
        v-if="!result"
        variant="primary"
        size="sm"
        :loading="busy"
        :disabled="!canConfirm"
        @click="confirm"
      >
        {{ t("startup.takeOverConfirm") }}
      </BaseButton>
    </template>
  </BaseModal>
</template>
