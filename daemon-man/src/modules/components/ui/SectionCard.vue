<script setup lang="ts">
import { cardClass } from "./form";

withDefaults(
  defineProps<{
    title?: string;
    description?: string;
    padded?: boolean;
    dense?: boolean;
  }>(),
  { padded: true, dense: false },
);
</script>

<template>
  <section :class="cardClass">
    <header
      v-if="title || $slots.actions"
      class="flex items-start justify-between gap-3 border-b border-zinc-200/80 px-4 py-3 dark:border-zinc-800/80"
    >
      <div class="min-w-0">
        <h2 class="text-sm font-semibold text-zinc-800 dark:text-zinc-100">
          {{ title }}
        </h2>
        <p
          v-if="description"
          class="mt-0.5 text-xs leading-relaxed text-zinc-500 dark:text-zinc-400"
        >
          {{ description }}
        </p>
      </div>
      <div v-if="$slots.actions" class="flex shrink-0 items-center gap-2">
        <slot name="actions" />
      </div>
    </header>
    <div :class="padded ? (dense ? 'p-3' : 'p-4') : ''">
      <slot />
    </div>
  </section>
</template>
