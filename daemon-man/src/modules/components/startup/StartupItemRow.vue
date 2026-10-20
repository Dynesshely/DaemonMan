<script setup lang="ts">
import { computed } from "vue";
import {
  CalendarClock,
  FolderOpen,
  KeyRound,
  RotateCcw,
  Server,
} from "lucide-vue-next";

import type { AutostartItem } from "@/api";
import BaseBadge from "@/modules/components/ui/BaseBadge.vue";
import BaseButton from "@/modules/components/ui/BaseButton.vue";
import ToggleSwitch from "@/modules/components/ui/ToggleSwitch.vue";
import { useI18n } from "@/modules/locales";
import { useIconStore } from "@/modules/stores/icons";
import type { BadgeVariant } from "@/modules/utils/status";

const props = defineProps<{
  item: AutostartItem;
  selected: boolean;
  selectable: boolean;
  pending?: boolean;
}>();

const emit = defineEmits<{
  "update:selected": [value: boolean];
  toggle: [item: AutostartItem, enabled: boolean];
  release: [item: AutostartItem];
  contextmenu: [item: AutostartItem, event: MouseEvent];
}>();

const { t } = useI18n();
const iconStore = useIconStore();

/** 20×20 行内图标；未加载或提取失败时为 null，回退到按来源的 lucide 图标。 */
const iconUrl = computed(() => iconStore.get(props.item.executable));

const fallbackIcon = computed(() => {
  switch (props.item.source) {
    case "startup-folder":
      return FolderOpen;
    case "scheduled-task":
      return CalendarClock;
    case "service":
      return Server;
    default:
      return KeyRound;
  }
});

const badges = computed<{ label: string; variant: BadgeVariant }[]>(() => {
  const list: { label: string; variant: BadgeVariant }[] = [];
  if (props.item.isSelf) {
    list.push({ label: t("startup.selfItem"), variant: "info" });
  }
  if (props.item.takenOver) {
    list.push({ label: t("startup.takenOver"), variant: "accent" });
  } else if (props.item.enabled) {
    list.push({ label: t("startup.enabled"), variant: "success" });
  } else {
    list.push({ label: t("startup.disabled"), variant: "neutral" });
  }
  if (!props.item.toggleSupported) {
    list.push({ label: t("startup.unsupported"), variant: "warn" });
  }
  if (props.item.requiresAdmin) {
    list.push({ label: t("startup.requiresAdmin"), variant: "warn" });
  }
  return list;
});

const toggleDisabled = computed(
  () =>
    !(props.item.toggleSupported && !props.item.isSelf) ||
    props.pending === true,
);

function onSelectChange(event: Event) {
  emit("update:selected", (event.target as HTMLInputElement).checked);
}

function onContextMenu(event: MouseEvent) {
  emit("contextmenu", props.item, event);
}
</script>

<template>
  <div
    class="flex items-start gap-3 px-4 py-3 transition"
    :class="
      selected
        ? 'bg-brand-50/50 dark:bg-brand-500/5'
        : 'hover:bg-zinc-50 dark:hover:bg-zinc-800/30'
    "
    @contextmenu.prevent="onContextMenu"
  >
    <input
      type="checkbox"
      class="mt-1 h-4 w-4 shrink-0 cursor-pointer rounded border-zinc-300 text-brand-600 accent-brand-600 disabled:cursor-not-allowed disabled:opacity-40 dark:border-zinc-600"
      :checked="selected"
      :disabled="!selectable"
      :aria-label="item.name"
      @change="onSelectChange"
    />

    <span
      class="mt-0.5 flex h-5 w-5 shrink-0 items-center justify-center"
      aria-hidden="true"
    >
      <img
        v-if="iconUrl"
        :src="iconUrl"
        alt=""
        class="h-5 w-5 rounded object-contain"
        draggable="false"
      />
      <component
        :is="fallbackIcon"
        v-else
        class="h-4 w-4 text-zinc-400 dark:text-zinc-500"
      />
    </span>

    <div class="min-w-0 flex-1">
      <div class="flex flex-wrap items-center gap-x-2 gap-y-1">
        <span
          class="max-w-full truncate text-sm font-medium text-zinc-800 dark:text-zinc-100"
          :title="item.name"
        >
          {{ item.name }}
        </span>
        <BaseBadge variant="neutral">{{ item.sourceLabel }}</BaseBadge>
        <BaseBadge
          v-for="badge in badges"
          :key="badge.label"
          :variant="badge.variant"
          :dot="badge.variant === 'success'"
        >
          {{ badge.label }}
        </BaseBadge>
      </div>

      <p
        class="mt-1 truncate font-mono text-[11px] leading-relaxed text-zinc-500 dark:text-zinc-400"
        :title="item.command"
      >
        {{ item.command }}
      </p>

      <p
        v-if="item.detail"
        class="mt-0.5 truncate text-[11px] text-zinc-400 dark:text-zinc-500"
        :title="item.detail"
      >
        {{ item.detail }}
      </p>

      <p
        v-if="item.source === 'service'"
        class="mt-1 text-[11px] text-amber-600 dark:text-amber-400"
      >
        {{ t("startup.serviceHint") }}
      </p>
    </div>

    <div class="flex shrink-0 flex-col items-end gap-2">
      <ToggleSwitch
        :model-value="item.enabled"
        :disabled="toggleDisabled"
        size="sm"
        :title="t('startup.toggle')"
        @update:model-value="emit('toggle', item, $event)"
      />
      <BaseButton
        v-if="item.takenOver"
        variant="secondary"
        size="sm"
        @click="emit('release', item)"
      >
        <RotateCcw class="h-3 w-3" />
        {{ t("startup.release") }}
      </BaseButton>
    </div>
  </div>
</template>
