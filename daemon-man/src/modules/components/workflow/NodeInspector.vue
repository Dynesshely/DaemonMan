<script setup lang="ts">
import { computed, ref } from "vue";
import { open } from "@tauri-apps/plugin-dialog";
import { FolderOpen, Play, Trash2 } from "lucide-vue-next";

import { api } from "@/api";
import type { WorkflowNode } from "@/api";
import BaseBadge from "@/modules/components/ui/BaseBadge.vue";
import BaseButton from "@/modules/components/ui/BaseButton.vue";
import FormField from "@/modules/components/ui/FormField.vue";
import { inputClass, textareaClass } from "@/modules/components/ui/form";
import { useI18n } from "@/modules/locales";
import { useUiStore } from "@/modules/stores/ui";
import { useWorkflowStore } from "@/modules/stores/workflows";
import { formatDuration } from "@/modules/utils/format";
import { nodeStatusVariant } from "@/modules/utils/status";

import { nodeKindMeta } from "./meta";
import NumberField from "./NumberField.vue";
import { errorText, fieldGroupClass, sectionLabelClass } from "./shared";
import ToggleRow from "./ToggleRow.vue";

const props = defineProps<{ node: WorkflowNode }>();
const emit = defineEmits<{ delete: [] }>();

const store = useWorkflowStore();
const ui = useUiStore();
const { t } = useI18n();

const meta = computed(() => nodeKindMeta[props.node.kind]);
const runState = computed(() => store.nodeRun(props.node.id));
const testing = ref(false);

const argsText = computed({
  get: () => props.node.app.args.join("\n"),
  set: (value: string) => {
    props.node.app.args = value
      .split(/\r?\n/)
      .map((line) => line.trim())
      .filter(Boolean);
  },
});

const envText = computed({
  get: () =>
    props.node.app.env
      .map((item) => `${item.key}=${item.value}`)
      .join("\n"),
  set: (value: string) => {
    props.node.app.env = value
      .split(/\r?\n/)
      .map((line) => line.trim())
      .filter(Boolean)
      .map((line) => {
        const index = line.indexOf("=");
        if (index < 0) return { key: line.trim(), value: "" };
        return {
          key: line.slice(0, index).trim(),
          value: line.slice(index + 1),
        };
      })
      .filter((item) => item.key.length > 0);
  },
});

/** `string | null` 字段 <-> 输入框文本。 */
function nullableText(
  get: () => string | null,
  set: (value: string | null) => void,
) {
  return computed({
    get: () => get() ?? "",
    set: (value: string) => set(value.trim() ? value : null),
  });
}

const appWorkingDir = nullableText(
  () => props.node.app.workingDir,
  (value) => {
    props.node.app.workingDir = value;
  },
);

const appProcessName = nullableText(
  () => props.node.app.processName,
  (value) => {
    props.node.app.processName = value;
  },
);

const scriptWorkingDir = nullableText(
  () => props.node.script.workingDir,
  (value) => {
    props.node.script.workingDir = value;
  },
);

const runDuration = computed(() => {
  const state = runState.value;
  if (!state?.startedAt) return "";
  const started = new Date(state.startedAt).getTime();
  const finished = state.finishedAt
    ? new Date(state.finishedAt).getTime()
    : Date.now();
  if (Number.isNaN(started) || Number.isNaN(finished)) return "";
  return formatDuration(Math.max(0, (finished - started) / 1000));
});

async function browsePath() {
  try {
    const selected = await open({
      multiple: false,
      directory: false,
      title: t("node.appPath"),
      filters: [
        {
          name: "Program",
          extensions: ["exe", "lnk", "bat", "cmd", "ps1"],
        },
      ],
    });
    if (typeof selected === "string" && selected.length > 0) {
      props.node.app.path = selected;
    }
  } catch (error) {
    ui.notify("error", errorText(error));
  }
}

async function testLaunch() {
  const path = props.node.app.path.trim();
  if (!path) {
    ui.notify("warn", t("node.executableRequired"));
    return;
  }
  testing.value = true;
  try {
    const pid = await api.testLaunch(
      path,
      props.node.app.args,
      props.node.app.workingDir ?? undefined,
    );
    ui.notify("success", t("workflow.testLaunchOk", { pid }));
  } catch (error) {
    ui.notify("error", `${t("workflow.testLaunch")}：${errorText(error)}`);
  } finally {
    testing.value = false;
  }
}
</script>

<template>
  <div class="space-y-4 p-4">
    <!-- 头部：类型 + 删除 -->
    <div class="flex items-start justify-between gap-2">
      <div class="flex min-w-0 items-center gap-2">
        <span
          class="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg"
          :class="meta.iconClass"
        >
          <component :is="meta.icon" class="h-4 w-4" />
        </span>
        <div class="min-w-0">
          <p class="truncate text-xs font-semibold text-zinc-800 dark:text-zinc-100">
            {{ t(meta.labelKey) }}
          </p>
          <p class="truncate font-mono text-[10px] text-zinc-400 dark:text-zinc-500">
            ID {{ props.node.id.slice(0, 8) }}
          </p>
        </div>
      </div>
      <BaseButton
        variant="ghost"
        size="sm"
        icon-only
        :title="t('workflow.deleteNode')"
        @click="emit('delete')"
      >
        <Trash2 class="h-4 w-4 text-red-500" />
      </BaseButton>
    </div>

    <!-- 运行状态 -->
    <div v-if="runState" :class="fieldGroupClass" class="space-y-1.5">
      <div class="flex items-center justify-between gap-2">
        <span :class="sectionLabelClass">{{ t("overview.lastRun") }}</span>
        <BaseBadge
          :variant="nodeStatusVariant(runState.status)"
          dot
          :pulse="runState.status === 'running'"
        >
          {{ t(`status.${runState.status}`) }}
        </BaseBadge>
      </div>
      <p
        v-if="runState.message"
        class="break-words text-[11px] leading-relaxed text-zinc-500 dark:text-zinc-400"
      >
        {{ runState.message }}
      </p>
      <div class="flex flex-wrap gap-x-3 gap-y-0.5 font-mono text-[10px] text-zinc-400 dark:text-zinc-500">
        <span v-if="runState.pid">PID {{ runState.pid }}</span>
        <span v-if="runDuration">耗时 {{ runDuration }}</span>
        <span v-if="runState.attempt > 1">{{ t("workflow.attempt", { n: runState.attempt }) }}</span>
      </div>
    </div>

    <!-- 名称 / 启用 -->
    <FormField :label="t('node.name')">
      <input v-model="props.node.name" :class="inputClass" :placeholder="t('node.name')" />
    </FormField>

    <ToggleRow
      v-model="props.node.enabled"
      :label="t('node.enabled')"
      :hint="props.node.enabled ? t('node.enabledHint') : t('workflow.disabledNode')"
    />

    <!-- 类型专属字段 -->
    <div class="space-y-3 border-t border-zinc-200/80 pt-3 dark:border-zinc-800/80">
      <p :class="sectionLabelClass">{{ t(meta.labelKey) }}</p>

      <template v-if="props.node.kind === 'application'">
        <FormField :label="t('node.appPath')" :hint="t('node.appPathPlaceholder')">
          <input v-model="props.node.app.path" :class="inputClass" class="font-mono! text-xs!" />
        </FormField>
        <div class="flex items-center gap-1.5">
          <BaseButton variant="secondary" size="sm" @click="browsePath">
            <FolderOpen class="h-3.5 w-3.5" />
            {{ t("common.browse") }}
          </BaseButton>
          <BaseButton
            variant="subtle"
            size="sm"
            :loading="testing"
            :disabled="!props.node.app.path"
            @click="testLaunch"
          >
            <Play v-if="!testing" class="h-3.5 w-3.5" />
            {{ t("workflow.testLaunch") }}
          </BaseButton>
        </div>

        <FormField :label="t('node.args')" :hint="t('node.argsHint')">
          <textarea v-model="argsText" :class="textareaClass" rows="2" spellcheck="false" />
        </FormField>

        <FormField :label="t('node.workingDir')">
          <input v-model="appWorkingDir" :class="inputClass" class="font-mono! text-xs!" />
        </FormField>

        <FormField :label="t('node.env')" :hint="t('node.envHint')">
          <textarea v-model="envText" :class="textareaClass" rows="2" spellcheck="false" />
        </FormField>

        <ToggleRow v-model="props.node.app.skipIfRunning" :label="t('node.skipIfRunning')" />
        <FormField :label="t('node.processName')" :hint="t('node.processNameHint')">
          <input v-model="appProcessName" :class="inputClass" class="font-mono! text-xs!" />
        </FormField>
        <ToggleRow v-model="props.node.app.waitForWindow" :label="t('node.waitForWindow')" />
        <NumberField
          v-if="props.node.app.waitForWindow"
          v-model="props.node.app.windowTimeoutMs"
          :label="t('node.windowTimeout')"
          :step="1000"
        />
        <ToggleRow
          v-model="props.node.app.closeMainWindow"
          :label="t('node.closeWindow')"
          :hint="t('node.closeWindowHint')"
        />
        <ToggleRow v-model="props.node.app.focusWindow" :label="t('node.focusWindow')" />
        <ToggleRow v-model="props.node.app.runAsAdmin" :label="t('node.runAsAdmin')" />
        <NumberField
          v-model="props.node.app.settleMs"
          :label="t('node.settle')"
          :step="100"
        />
      </template>

      <template v-else-if="props.node.kind === 'delay'">
        <NumberField
          v-model="props.node.delayMs"
          :label="t('node.delay')"
          :step="100"
        />
      </template>

      <template v-else-if="props.node.kind === 'wait'">
        <FormField :label="t('node.waitProcess')">
          <input v-model="props.node.wait.processName" :class="inputClass" class="font-mono! text-xs!" />
        </FormField>
        <ToggleRow
          v-model="props.node.wait.windowVisible"
          :label="t('node.waitWindowVisible')"
        />
        <NumberField
          v-model="props.node.wait.timeoutMs"
          :label="t('node.waitTimeout')"
          :step="1000"
        />
        <ToggleRow
          v-model="props.node.wait.failOnTimeout"
          :label="t('node.waitFailOnTimeout')"
        />
      </template>

      <template v-else>
        <FormField :label="t('node.scriptCommand')" :hint="t('node.scriptCommandHint')">
          <textarea v-model="props.node.script.command" :class="textareaClass" rows="3" spellcheck="false" />
        </FormField>
        <FormField :label="t('node.scriptWorkingDir')">
          <input v-model="scriptWorkingDir" :class="inputClass" class="font-mono! text-xs!" />
        </FormField>
        <NumberField
          v-model="props.node.script.timeoutMs"
          :label="t('node.scriptTimeout')"
          :step="1000"
        />
        <ToggleRow
          v-model="props.node.script.failOnNonzero"
          :label="t('node.scriptFailOnNonzero')"
        />
      </template>
    </div>

    <!-- 备注 / 位置 -->
    <div class="space-y-3 border-t border-zinc-200/80 pt-3 dark:border-zinc-800/80">
      <FormField :label="t('node.note')">
        <textarea v-model="props.node.note" :class="textareaClass" rows="2" />
      </FormField>
      <p class="font-mono text-[10px] text-zinc-400 dark:text-zinc-500">
        {{ t("node.position") }} · x {{ Math.round(props.node.position.x) }} ·
        y {{ Math.round(props.node.position.y) }}
      </p>
    </div>
  </div>
</template>
