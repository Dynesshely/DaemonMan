<script setup lang="ts">
defineProps<{
  title: string;
  description?: string;
}>();
</script>

<template>
  <div class="mb-4 flex flex-wrap items-end justify-between gap-3">
    <div class="min-w-0">
      <h1 class="text-lg font-semibold tracking-tight text-zinc-900 dark:text-zinc-50">
        {{ title }}
      </h1>
      <p
        v-if="description"
        class="mt-1 text-xs leading-relaxed text-zinc-500 dark:text-zinc-400"
      >
        {{ description }}
      </p>
    </div>
    <div v-if="$slots.actions" class="flex shrink-0 items-center gap-2">
      <slot name="actions" />
    </div>
  </div>
</template>
