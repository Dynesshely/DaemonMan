<script setup lang="ts">
import { Inbox } from "lucide-vue-next";

withDefaults(
  defineProps<{
    title?: string;
    description?: string;
    compact?: boolean;
  }>(),
  { compact: false },
);
</script>

<template>
  <div
    class="flex flex-col items-center justify-center text-center"
    :class="compact ? 'gap-2 py-8' : 'gap-3 py-14'"
  >
    <div
      class="flex items-center justify-center rounded-2xl bg-zinc-100 text-zinc-400 dark:bg-zinc-800/70 dark:text-zinc-500"
      :class="compact ? 'h-10 w-10' : 'h-14 w-14'"
    >
      <slot name="icon">
        <Inbox :class="compact ? 'h-5 w-5' : 'h-7 w-7'" />
      </slot>
    </div>
    <p
      v-if="title"
      class="text-sm font-medium text-zinc-700 dark:text-zinc-200"
    >
      {{ title }}
    </p>
    <p
      v-if="description"
      class="max-w-sm text-xs leading-relaxed text-zinc-500 dark:text-zinc-400"
    >
      {{ description }}
    </p>
    <div v-if="$slots.action" class="mt-1">
      <slot name="action" />
    </div>
  </div>
</template>
