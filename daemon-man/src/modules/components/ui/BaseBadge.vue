<script setup lang="ts">
import { computed } from "vue";

import type { BadgeVariant } from "@/modules/utils/status";

const props = withDefaults(
  defineProps<{
    variant?: BadgeVariant;
    dot?: boolean;
    pulse?: boolean;
  }>(),
  { variant: "neutral", dot: false, pulse: false },
);

const variantClasses: Record<BadgeVariant, string> = {
  neutral:
    "bg-zinc-100 text-zinc-600 ring-zinc-200 dark:bg-zinc-800 dark:text-zinc-300 dark:ring-zinc-700",
  accent:
    "bg-brand-50 text-brand-700 ring-brand-200 dark:bg-brand-500/15 dark:text-brand-300 dark:ring-brand-500/30",
  success:
    "bg-emerald-50 text-emerald-700 ring-emerald-200 dark:bg-emerald-500/15 dark:text-emerald-300 dark:ring-emerald-500/30",
  warn: "bg-amber-50 text-amber-700 ring-amber-200 dark:bg-amber-500/15 dark:text-amber-300 dark:ring-amber-500/30",
  danger:
    "bg-red-50 text-red-700 ring-red-200 dark:bg-red-500/15 dark:text-red-300 dark:ring-red-500/30",
  info: "bg-sky-50 text-sky-700 ring-sky-200 dark:bg-sky-500/15 dark:text-sky-300 dark:ring-sky-500/30",
  running:
    "bg-brand-50 text-brand-700 ring-brand-200 dark:bg-brand-500/15 dark:text-brand-300 dark:ring-brand-500/30",
  skipped:
    "bg-teal-50 text-teal-700 ring-teal-200 dark:bg-teal-500/15 dark:text-teal-300 dark:ring-teal-500/30",
};

const dotClasses: Record<BadgeVariant, string> = {
  neutral: "bg-zinc-400",
  accent: "bg-brand-500",
  success: "bg-emerald-500",
  warn: "bg-amber-500",
  danger: "bg-red-500",
  info: "bg-sky-500",
  running: "bg-brand-500",
  skipped: "bg-teal-500",
};

const classes = computed(() => [
  "inline-flex items-center gap-1.5 rounded-full px-2 py-0.5 text-[11px] font-medium ring-1 ring-inset",
  variantClasses[props.variant],
]);
</script>

<template>
  <span :class="classes">
    <span
      v-if="dot"
      class="h-1.5 w-1.5 shrink-0 rounded-full"
      :class="[dotClasses[variant], pulse ? 'animate-pulse' : '']"
    />
    <slot />
  </span>
</template>
