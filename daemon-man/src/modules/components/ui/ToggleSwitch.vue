<script setup lang="ts">
const props = withDefaults(
  defineProps<{
    modelValue: boolean;
    disabled?: boolean;
    size?: "sm" | "md";
  }>(),
  { disabled: false, size: "md" },
);

const emit = defineEmits<{ "update:modelValue": [value: boolean] }>();

function toggle() {
  if (props.disabled) return;
  emit("update:modelValue", !props.modelValue);
}
</script>

<template>
  <button
    type="button"
    role="switch"
    :aria-checked="modelValue"
    :disabled="disabled"
    class="relative inline-flex shrink-0 items-center rounded-full transition-colors duration-200 outline-none focus-visible:ring-2 focus-visible:ring-brand-500/50 disabled:cursor-not-allowed disabled:opacity-50"
    :class="[
      size === 'sm' ? 'h-5 w-9' : 'h-6 w-11',
      modelValue
        ? 'bg-brand-600'
        : 'bg-zinc-300 dark:bg-zinc-700',
    ]"
    @click="toggle"
  >
    <span
      class="inline-block rounded-full bg-white shadow-sm transition-transform duration-200"
      :class="[
        size === 'sm' ? 'h-4 w-4' : 'h-5 w-5',
        modelValue
          ? size === 'sm'
            ? 'translate-x-[18px]'
            : 'translate-x-[22px]'
          : 'translate-x-[3px]',
      ]"
    />
  </button>
</template>
