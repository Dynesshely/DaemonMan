<script setup lang="ts">
import { labelClass, hintClass } from "./form";

defineProps<{
  label?: string;
  hint?: string;
  error?: string;
}>();
</script>

<template>
  <label class="flex flex-col gap-1.5">
    <span v-if="label" :class="labelClass">{{ label }}</span>
    <slot />
    <span v-if="error" class="text-xs text-red-600 dark:text-red-400">
      {{ error }}
    </span>
    <span v-else-if="hint" :class="hintClass">{{ hint }}</span>
  </label>
</template>
