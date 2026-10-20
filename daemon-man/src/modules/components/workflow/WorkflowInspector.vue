<script setup lang="ts">
import { computed } from "vue";
import { MousePointerClick } from "lucide-vue-next";

import BaseBadge from "@/modules/components/ui/BaseBadge.vue";
import EmptyState from "@/modules/components/ui/EmptyState.vue";
import { useI18n } from "@/modules/locales";
import { useWorkflowStore } from "@/modules/stores/workflows";

import EdgeInspector from "./EdgeInspector.vue";
import NodeInspector from "./NodeInspector.vue";
import { panelClass, panelHeaderClass } from "./shared";

const props = defineProps<{
  nodeId: string | null;
  edgeId: string | null;
}>();

const emit = defineEmits<{ "delete-node": []; "delete-edge": [] }>();

const store = useWorkflowStore();
const { t } = useI18n();

const node = computed(() => {
  if (!props.nodeId) return null;
  return store.draft?.nodes.find((item) => item.id === props.nodeId) ?? null;
});

const edge = computed(() => {
  if (!props.edgeId) return null;
  return store.draft?.edges.find((item) => item.id === props.edgeId) ?? null;
});

const title = computed(() =>
  edge.value && !node.value
    ? t("workflow.edgeCondition")
    : t("workflow.inspector"),
);

const conditionKey = computed(() => {
  switch (edge.value?.condition) {
    case "failure":
      return "workflow.edgeFailure";
    case "always":
      return "workflow.edgeAlways";
    default:
      return "workflow.edgeSuccess";
  }
});
</script>

<template>
  <aside :class="panelClass">
    <header :class="panelHeaderClass">
      <p class="truncate text-xs font-semibold text-zinc-700 dark:text-zinc-200">
        {{ title }}
      </p>
      <BaseBadge v-if="node" variant="neutral">
        {{ t(`node.kind.${node.kind}`) }}
      </BaseBadge>
      <BaseBadge v-else-if="edge" variant="accent">
        {{ t(conditionKey) }}
      </BaseBadge>
    </header>

    <div class="min-h-0 flex-1 overflow-y-auto">
      <NodeInspector
        v-if="node"
        :node="node"
        @delete="emit('delete-node')"
      />
      <EdgeInspector
        v-else-if="edge"
        :edge="edge"
        @delete="emit('delete-edge')"
      />
      <div v-else class="p-4">
        <EmptyState
          :title="t('workflow.inspectorHint')"
          :description="t('workflow.connectHint')"
          compact
        >
          <template #icon>
            <MousePointerClick class="h-5 w-5" />
          </template>
        </EmptyState>
      </div>
    </div>
  </aside>
</template>
