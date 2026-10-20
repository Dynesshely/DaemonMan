<script setup lang="ts">
import { Loader2 } from "lucide-vue-next";

withDefaults(defineProps<{ size?: "sm" | "md" | "lg" }>(), { size: "md" });
</script>

<template>
  <Loader2
    class="animate-spin text-brand-500"
    :class="{
      'h-3.5 w-3.5': size === 'sm',
      'h-5 w-5': size === 'md',
      'h-7 w-7': size === 'lg',
    }"
  />
</template>
