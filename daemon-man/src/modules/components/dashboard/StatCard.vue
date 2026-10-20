<script setup lang="ts">
import { computed } from "vue";
import type { Component } from "vue";

import { cardClass } from "@/modules/components/ui/form";

const props = withDefaults(
  defineProps<{
    label: string;
    value: string | number;
    hint?: string;
    icon: Component;
    tone?: "brand" | "emerald" | "amber" | "sky" | "zinc";
  }>(),
  { tone: "brand" },
);

const toneClasses: Record<string, string> = {
  brand:
    "bg-brand-50 text-brand-600 ring-brand-100 dark:bg-brand-500/15 dark:text-brand-300 dark:ring-brand-500/20",
  emerald:
    "bg-emerald-50 text-emerald-600 ring-emerald-100 dark:bg-emerald-500/15 dark:text-emerald-300 dark:ring-emerald-500/20",
  amber:
    "bg-amber-50 text-amber-600 ring-amber-100 dark:bg-amber-500/15 dark:text-amber-300 dark:ring-amber-500/20",
  sky: "bg-sky-50 text-sky-600 ring-sky-100 dark:bg-sky-500/15 dark:text-sky-300 dark:ring-sky-500/20",
  zinc: "bg-zinc-100 text-zinc-600 ring-zinc-200 dark:bg-zinc-800 dark:text-zinc-300 dark:ring-zinc-700",
};

const iconClasses = computed(() => toneClasses[props.tone] ?? toneClasses.brand);
</script>

<template>
  <div :class="[cardClass, 'flex items-center gap-3 px-4 py-3.5']">
    <div
      class="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl ring-1 ring-inset"
      :class="iconClasses"
    >
      <component :is="icon" class="h-5 w-5" />
    </div>
    <div class="min-w-0">
      <p
        class="truncate text-[11px] font-semibold uppercase tracking-wider text-zinc-500 dark:text-zinc-400"
      >
        {{ label }}
      </p>
      <p
        class="tabular mt-0.5 truncate text-lg font-semibold leading-tight text-zinc-900 dark:text-zinc-50"
      >
        {{ value }}
      </p>
      <p
        v-if="hint"
        class="mt-0.5 truncate text-[11px] text-zinc-500 dark:text-zinc-400"
      >
        {{ hint }}
      </p>
    </div>
  </div>
</template>
