<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from "vue";
import { FileCode2 } from "lucide-vue-next";

import type { AutostartItem, ScriptContent } from "@/api";
import { api } from "@/api";
import BaseBadge from "@/modules/components/ui/BaseBadge.vue";
import BaseButton from "@/modules/components/ui/BaseButton.vue";
import BaseModal from "@/modules/components/ui/BaseModal.vue";
import BaseSpinner from "@/modules/components/ui/BaseSpinner.vue";
import { inputClass } from "@/modules/components/ui/form";
import { useI18n } from "@/modules/locales";
import { useUiStore } from "@/modules/stores/ui";
import { formatBytes } from "@/modules/utils/format";

const props = withDefaults(
  defineProps<{
    open: boolean;
    item: AutostartItem | null;
  }>(),
  { item: null },
);

const emit = defineEmits<{
  close: [];
  saved: [content: ScriptContent];
}>();

const { t } = useI18n();
const ui = useUiStore();

/** 编辑器文本框：与 textareaClass 一致的等宽风格，但更高（避免两套 min-h 冲突）。 */
const editorClass = [
  inputClass,
  "min-h-[320px] max-h-[52vh] resize-y font-mono text-xs leading-relaxed",
].join(" ");

const loading = ref(false);
const loadError = ref<string | null>(null);
const saving = ref(false);
const script = ref<ScriptContent | null>(null);
const content = ref("");
const savedContent = ref("");

const dirty = computed(() => content.value !== savedContent.value);
const canSave = computed(
  () =>
    !loading.value &&
    !saving.value &&
    !loadError.value &&
    Boolean(script.value) &&
    dirty.value,
);

async function load() {
  const current = props.item;
  if (!current) return;
  loading.value = true;
  loadError.value = null;
  script.value = null;
  content.value = "";
  savedContent.value = "";
  try {
    const data = await api.readAutostartScript(current.id);
    script.value = data;
    content.value = data.content;
    savedContent.value = data.content;
  } catch (error) {
    loadError.value = String(error);
    ui.notify(
      "error",
      t("startup.editLoadFailed", { error: String(error) }),
    );
  } finally {
    loading.value = false;
  }
}

async function save() {
  const current = props.item;
  if (!current || !canSave.value) return;
  saving.value = true;
  try {
    const updated = await api.writeAutostartScript(
      current.id,
      content.value,
      script.value?.encoding,
    );
    script.value = updated;
    savedContent.value = content.value;
    ui.notify("success", t("startup.editSaved"));
    emit("saved", updated);
  } catch (error) {
    ui.notify(
      "error",
      t("startup.editSaveFailed", { error: String(error) }),
    );
  } finally {
    saving.value = false;
  }
}

function requestClose() {
  if (saving.value) return;
  if (dirty.value) {
    const confirmed = window.confirm("当前修改尚未保存，确定放弃？");
    if (!confirmed) return;
  }
  emit("close");
}

function onKeydown(event: KeyboardEvent) {
  if (!props.open) return;
  if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "s") {
    event.preventDefault();
    void save();
  }
}

watch(
  () => [props.open, props.item?.id] as const,
  ([open]) => {
    if (open) void load();
  },
  { immediate: true },
);

watch(
  () => props.open,
  (open) => {
    if (open) {
      window.addEventListener("keydown", onKeydown);
    } else {
      window.removeEventListener("keydown", onKeydown);
    }
  },
  { immediate: true },
);

onBeforeUnmount(() => window.removeEventListener("keydown", onKeydown));
</script>

<template>
  <BaseModal
    :open="open"
    :title="t('startup.editTitle')"
    size="xl"
    :closable="!saving"
    @close="requestClose"
  >
    <div v-if="loading" class="flex flex-col items-center justify-center gap-3 py-16">
      <BaseSpinner size="md" />
      <p class="text-xs text-zinc-500 dark:text-zinc-400">
        {{ t("common.loading") }}
      </p>
    </div>

    <div v-else-if="loadError" class="flex flex-col items-start gap-3 py-6">
      <p class="text-xs leading-relaxed text-red-600 dark:text-red-400">
        {{ t("startup.editLoadFailed", { error: loadError ?? "" }) }}
      </p>
      <BaseButton variant="secondary" size="sm" @click="load">
        {{ t("common.retry") }}
      </BaseButton>
    </div>

    <div v-else class="flex flex-col gap-3">
      <div class="flex flex-wrap items-center gap-2">
        <FileCode2 class="h-4 w-4 shrink-0 text-zinc-400 dark:text-zinc-500" />
        <span
          class="min-w-0 max-w-full truncate font-mono text-[11px] text-zinc-600 dark:text-zinc-300"
          :title="script?.path"
        >
          {{ t("startup.editPath") }}：{{ script?.path ?? item?.executable ?? "—" }}
        </span>
        <BaseBadge v-if="script" variant="neutral">
          {{ t("startup.editEncoding", { encoding: script.encoding }) }}
        </BaseBadge>
        <BaseBadge :variant="dirty ? 'warn' : 'success'">
          {{ dirty ? t("common.unsaved") : t("common.saved") }}
        </BaseBadge>
        <span
          v-if="script"
          class="tabular ml-auto shrink-0 text-[11px] text-zinc-400 dark:text-zinc-500"
        >
          {{ formatBytes(script.size) }}
        </span>
      </div>

      <p class="text-[11px] leading-relaxed text-zinc-500 dark:text-zinc-400">
        {{ t("startup.editHint") }}
      </p>

      <textarea
        v-model="content"
        :class="editorClass"
        spellcheck="false"
        autocomplete="off"
        autocapitalize="off"
        wrap="off"
        :aria-label="t('startup.editTitle')"
      />

      <p class="text-[11px] text-zinc-400 dark:text-zinc-500">
        Ctrl / Cmd + S
      </p>
    </div>

    <template #footer>
      <BaseButton
        variant="secondary"
        size="sm"
        :disabled="saving"
        @click="requestClose"
      >
        {{ t("common.close") }}
      </BaseButton>
      <BaseButton
        variant="primary"
        size="sm"
        :loading="saving"
        :disabled="!canSave"
        @click="save"
      >
        {{ t("common.save") }}
      </BaseButton>
    </template>
  </BaseModal>
</template>
