<script setup lang="ts">
import ToggleSwitch from "@/modules/components/ui/ToggleSwitch.vue";

defineProps<{
  label: string;
  hint?: string;
  modelValue: boolean;
  disabled?: boolean;
}>();

const emit = defineEmits<{ "update:modelValue": [value: boolean] }>();
</script>

<template>
  <div class="flex items-center justify-between gap-3">
    <div class="min-w-0">
      <p class="text-xs leading-relaxed text-zinc-600 dark:text-zinc-300">
        {{ label }}
      </p>
      <p
        v-if="hint"
        class="mt-0.5 text-[11px] leading-relaxed text-zinc-400 dark:text-zinc-500"
      >
        {{ hint }}
      </p>
    </div>
    <ToggleSwitch
      :model-value="modelValue"
      :disabled="disabled"
      size="sm"
      @update:model-value="emit('update:modelValue', $event)"
    />
  </div>
</template>
