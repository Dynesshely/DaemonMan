<script setup lang="ts">
interface TabOption {
  value: string;
  label: string;
  count?: number;
}

withDefaults(
  defineProps<{
    modelValue: string;
    options: TabOption[];
    disabled?: boolean;
  }>(),
  { disabled: false },
);

const emit = defineEmits<{ "update:modelValue": [value: string] }>();
</script>

<template>
  <div class="flex flex-wrap items-center gap-1" role="group">
    <button
      v-for="option in options"
      :key="option.value"
      type="button"
      class="inline-flex items-center gap-1.5 rounded-full border px-2.5 py-1 text-[11px] font-medium outline-none transition focus-visible:ring-2 focus-visible:ring-brand-500/50 disabled:cursor-not-allowed disabled:opacity-60"
      :class="
        modelValue === option.value
          ? 'border-brand-600 bg-brand-600 text-white shadow-sm shadow-brand-600/25 dark:border-brand-500 dark:bg-brand-500'
          : 'border-zinc-200 bg-white text-zinc-600 hover:border-zinc-300 hover:bg-zinc-50 dark:border-zinc-700 dark:bg-zinc-900 dark:text-zinc-300 dark:hover:border-zinc-600 dark:hover:bg-zinc-800'
      "
      :aria-pressed="modelValue === option.value"
      :disabled="disabled"
      @click="emit('update:modelValue', option.value)"
    >
      <span>{{ option.label }}</span>
      <span
        v-if="option.count !== undefined"
        class="tabular rounded-full px-1.5 text-[10px] leading-4"
        :class="
          modelValue === option.value
            ? 'bg-white/20 text-white'
            : 'bg-zinc-100 text-zinc-500 dark:bg-zinc-800 dark:text-zinc-400'
        "
      >
        {{ option.count }}
      </span>
    </button>
  </div>
</template>
