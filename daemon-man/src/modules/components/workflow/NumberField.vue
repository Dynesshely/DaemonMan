<script setup lang="ts">
import FormField from "@/modules/components/ui/FormField.vue";
import { inputClass } from "@/modules/components/ui/form";

const props = withDefaults(
  defineProps<{
    label: string;
    hint?: string;
    modelValue: number;
    min?: number;
    max?: number;
    step?: number;
  }>(),
  { min: 0, max: undefined, step: 100 },
);

const emit = defineEmits<{ "update:modelValue": [value: number] }>();

function onInput(event: Event) {
  const raw = (event.target as HTMLInputElement).value;
  const parsed = Number(raw);
  let value = Number.isFinite(parsed) ? Math.round(parsed) : props.min;
  value = Math.max(props.min, value);
  if (typeof props.max === "number") value = Math.min(props.max, value);
  emit("update:modelValue", value);
}
</script>

<template>
  <FormField :label="label" :hint="hint">
    <input
      type="number"
      :value="modelValue"
      :min="min"
      :max="max"
      :step="step"
      :class="inputClass"
      class="tabular"
      @input="onInput"
    />
  </FormField>
</template>
