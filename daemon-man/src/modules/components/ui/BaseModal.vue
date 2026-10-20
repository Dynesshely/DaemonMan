<script setup lang="ts">
import { onBeforeUnmount, watch } from "vue";
import { X } from "lucide-vue-next";

import BaseButton from "./BaseButton.vue";

const props = withDefaults(
  defineProps<{
    open: boolean;
    title?: string;
    description?: string;
    size?: "sm" | "md" | "lg" | "xl";
    closable?: boolean;
  }>(),
  { size: "md", closable: true },
);

const emit = defineEmits<{ close: [] }>();

const sizeClasses: Record<string, string> = {
  sm: "max-w-md",
  md: "max-w-lg",
  lg: "max-w-2xl",
  xl: "max-w-4xl",
};

function onKeydown(event: KeyboardEvent) {
  if (event.key === "Escape" && props.closable) {
    emit("close");
  }
}

watch(
  () => props.open,
  (open) => {
    if (open) {
      window.addEventListener("keydown", onKeydown);
    } else {
      window.removeEventListener("keydown", onKeydown);
    }
  },
);

onBeforeUnmount(() => window.removeEventListener("keydown", onKeydown));
</script>

<template>
  <Teleport to="body">
    <Transition
      enter-active-class="transition duration-150 ease-out"
      enter-from-class="opacity-0"
      enter-to-class="opacity-100"
      leave-active-class="transition duration-100 ease-in"
      leave-from-class="opacity-100"
      leave-to-class="opacity-0"
    >
      <div
        v-if="open"
        class="fixed inset-0 z-50 flex items-center justify-center p-4"
      >
        <div
          class="absolute inset-0 bg-zinc-950/50 backdrop-blur-[2px]"
          @click="closable && emit('close')"
        />
        <div
          class="relative flex max-h-[85vh] w-full flex-col overflow-hidden rounded-2xl border border-zinc-200 bg-white shadow-2xl dark:border-zinc-800 dark:bg-zinc-900"
          :class="sizeClasses[size]"
          role="dialog"
          aria-modal="true"
        >
          <header
            class="flex items-start justify-between gap-4 border-b border-zinc-200 px-5 py-4 dark:border-zinc-800"
          >
            <div class="min-w-0">
              <h2 class="text-base font-semibold text-zinc-900 dark:text-zinc-100">
                {{ title }}
              </h2>
              <p
                v-if="description"
                class="mt-1 text-xs leading-relaxed text-zinc-500 dark:text-zinc-400"
              >
                {{ description }}
              </p>
            </div>
            <BaseButton
              v-if="closable"
              variant="ghost"
              size="sm"
              icon-only
              @click="emit('close')"
            >
              <X class="h-4 w-4" />
            </BaseButton>
          </header>

          <div class="min-h-0 flex-1 overflow-y-auto px-5 py-4">
            <slot />
          </div>

          <footer
            v-if="$slots.footer"
            class="flex items-center justify-end gap-2 border-t border-zinc-200 bg-zinc-50/70 px-5 py-3 dark:border-zinc-800 dark:bg-zinc-950/40"
          >
            <slot name="footer" />
          </footer>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>
