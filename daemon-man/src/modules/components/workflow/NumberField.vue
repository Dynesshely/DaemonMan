<script setup lang="ts">
import FormField from "@/modules/components/ui/FormField.vue";
import { inputClass } from "@/modules/components/ui/form";

const props = withDefaults(
  defineProps<{
    label: string;
    hint?: string;
    modelValue: number;
    min?: number;
    step?: number;
  }>(),
  { min: 0, step: 100 },
);

const emit = defineEmits<{ "update:modelValue": [value: number] }>();

function onInput(event: Event) {
  const raw = (event.target as HTMLInputElement).value;
  const parsed = Number(raw);
  emit(
    "update:modelValue",
    Number.isFinite(parsed) ? Math.max(props.min, Math.round(parsed)) : props.min,
  );
}
</script>

<template>
  <FormField :label="label" :hint="hint">
    <input
      type="number"
      :value="modelValue"
      :min="min"
      :step="step"
      :class="inputClass"
      class="tabular"
      @input="onInput"
    />
  </FormField>
</template>
