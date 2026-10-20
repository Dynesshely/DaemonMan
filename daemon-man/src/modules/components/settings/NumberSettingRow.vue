<script setup lang="ts">
import { ref, watch } from "vue";

import { inputClass } from "@/modules/components/ui/form";

const props = withDefaults(
  defineProps<{
    label: string;
    description?: string;
    modelValue: number;
    min?: number;
    max?: number;
    step?: number;
    suffix?: string;
    disabled?: boolean;
  }>(),
  { min: 0, step: 1, disabled: false },
);

const emit = defineEmits<{ commit: [value: number] }>();

const text = ref(String(props.modelValue));

watch(
  () => props.modelValue,
  (value) => {
    text.value = String(value);
  },
);

function commit() {
  const parsed = Number(text.value);
  if (!Number.isFinite(parsed)) {
    text.value = String(props.modelValue);
    return;
  }
  const min = props.min ?? 0;
  const max = props.max ?? Number.MAX_SAFE_INTEGER;
  const clamped = Math.min(max, Math.max(min, Math.round(parsed)));
  text.value = String(clamped);
  if (clamped !== props.modelValue) {
    emit("commit", clamped);
  }
}

function onKeydown(event: KeyboardEvent) {
  if (event.key === "Enter") {
    (event.target as HTMLInputElement).blur();
  }
}
</script>

<template>
  <div class="flex items-start justify-between gap-4 py-3">
    <div class="min-w-0">
      <p class="text-sm font-medium text-zinc-800 dark:text-zinc-100">
        {{ label }}
      </p>
      <p
        v-if="description"
        class="mt-0.5 text-xs leading-relaxed text-zinc-500 dark:text-zinc-400"
      >
        {{ description }}
      </p>
    </div>
    <div class="flex shrink-0 items-center gap-2">
      <input
        v-model="text"
        :class="[inputClass, 'tabular w-32 text-right']"
        type="number"
        :min="min"
        :max="max"
        :step="step"
        :disabled="disabled"
        inputmode="numeric"
        @change="commit"
        @blur="commit"
        @keydown="onKeydown"
      />
      <span
        v-if="suffix"
        class="w-10 shrink-0 text-[11px] text-zinc-500 dark:text-zinc-400"
      >
        {{ suffix }}
      </span>
    </div>
  </div>
</template>
