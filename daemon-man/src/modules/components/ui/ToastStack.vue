<script setup lang="ts">
import {
  AlertTriangle,
  CheckCircle2,
  Info,
  X,
  XCircle,
} from "lucide-vue-next";

import { useUiStore, type ToastKind } from "@/modules/stores/ui";

const ui = useUiStore();

const icons: Record<ToastKind, unknown> = {
  info: Info,
  success: CheckCircle2,
  warn: AlertTriangle,
  error: XCircle,
};

const accents: Record<ToastKind, string> = {
  info: "border-sky-200 bg-sky-50 text-sky-800 dark:border-sky-500/30 dark:bg-sky-500/10 dark:text-sky-200",
  success:
    "border-emerald-200 bg-emerald-50 text-emerald-800 dark:border-emerald-500/30 dark:bg-emerald-500/10 dark:text-emerald-200",
  warn: "border-amber-200 bg-amber-50 text-amber-900 dark:border-amber-500/30 dark:bg-amber-500/10 dark:text-amber-200",
  error:
    "border-red-200 bg-red-50 text-red-800 dark:border-red-500/30 dark:bg-red-500/10 dark:text-red-200",
};
</script>

<template>
  <div
    class="pointer-events-none fixed bottom-10 right-4 z-[60] flex w-80 flex-col gap-2"
  >
    <TransitionGroup
      enter-active-class="transition duration-150 ease-out"
      enter-from-class="translate-y-2 opacity-0"
      enter-to-class="translate-y-0 opacity-100"
      leave-active-class="transition duration-100 ease-in"
      leave-from-class="opacity-100"
      leave-to-class="opacity-0"
    >
      <div
        v-for="toast in ui.toasts"
        :key="toast.id"
        class="pointer-events-auto flex items-start gap-2.5 rounded-xl border px-3.5 py-2.5 text-xs shadow-lg backdrop-blur"
        :class="accents[toast.kind]"
      >
        <component :is="icons[toast.kind]" class="mt-0.5 h-4 w-4 shrink-0" />
        <p class="min-w-0 flex-1 leading-relaxed break-words">
          {{ toast.message }}
        </p>
        <button
          type="button"
          class="shrink-0 rounded p-0.5 opacity-60 transition hover:opacity-100"
          @click="ui.dismiss(toast.id)"
        >
          <X class="h-3.5 w-3.5" />
        </button>
      </div>
    </TransitionGroup>
  </div>
</template>
