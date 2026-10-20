<script setup lang="ts">
import ToggleSwitch from "@/modules/components/ui/ToggleSwitch.vue";

withDefaults(
  defineProps<{
    label: string;
    description?: string;
    modelValue: boolean;
    disabled?: boolean;
    pending?: boolean;
  }>(),
  { disabled: false, pending: false },
);

const emit = defineEmits<{ "update:modelValue": [value: boolean] }>();
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
      <span
        v-if="pending"
        class="text-[11px] text-zinc-400 dark:text-zinc-500"
      >
        {{ "…" }}
      </span>
      <ToggleSwitch
        :model-value="modelValue"
        :disabled="disabled || pending"
        @update:model-value="emit('update:modelValue', $event)"
      />
    </div>
  </div>
</template>
