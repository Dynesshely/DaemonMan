<script setup lang="ts">
import { computed } from "vue";
import { Loader2 } from "lucide-vue-next";

const props = withDefaults(
  defineProps<{
    variant?: "primary" | "secondary" | "ghost" | "danger" | "subtle";
    size?: "sm" | "md" | "lg";
    type?: "button" | "submit" | "reset";
    disabled?: boolean;
    loading?: boolean;
    block?: boolean;
    iconOnly?: boolean;
  }>(),
  {
    variant: "secondary",
    size: "md",
    type: "button",
    disabled: false,
    loading: false,
    block: false,
    iconOnly: false,
  },
);

const variantClasses: Record<string, string> = {
  primary:
    "bg-brand-600 text-white shadow-sm shadow-brand-600/25 hover:bg-brand-500 active:bg-brand-700",
  secondary:
    "border border-zinc-300 bg-white text-zinc-700 hover:bg-zinc-50 active:bg-zinc-100 dark:border-zinc-700 dark:bg-zinc-900 dark:text-zinc-200 dark:hover:bg-zinc-800",
  ghost:
    "text-zinc-600 hover:bg-zinc-200/70 active:bg-zinc-300/70 dark:text-zinc-300 dark:hover:bg-zinc-800",
  danger:
    "bg-red-600 text-white shadow-sm shadow-red-600/20 hover:bg-red-500 active:bg-red-700",
  subtle:
    "bg-brand-50 text-brand-700 hover:bg-brand-100 dark:bg-brand-500/15 dark:text-brand-300 dark:hover:bg-brand-500/25",
};

const sizeClasses: Record<string, string> = {
  sm: "h-8 gap-1.5 px-2.5 text-xs",
  md: "h-9 gap-2 px-3.5 text-sm",
  lg: "h-11 gap-2 px-5 text-sm",
};

const classes = computed(() => [
  "inline-flex shrink-0 items-center justify-center rounded-lg font-medium transition outline-none focus-visible:ring-2 focus-visible:ring-brand-500/50 disabled:cursor-not-allowed disabled:opacity-50",
  variantClasses[props.variant],
  sizeClasses[props.size],
  props.iconOnly
    ? props.size === "sm"
      ? "w-8 px-0"
      : "w-9 px-0"
    : "",
  props.block ? "w-full" : "",
]);
</script>

<template>
  <button
    :type="type"
    :disabled="disabled || loading"
    :class="classes"
    :aria-busy="loading"
  >
    <Loader2 v-if="loading" class="h-4 w-4 animate-spin" />
    <slot />
  </button>
</template>
