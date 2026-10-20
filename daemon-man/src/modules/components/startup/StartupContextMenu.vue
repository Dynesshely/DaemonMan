<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from "vue";
import { useRoute } from "vue-router";
import {
  Ban,
  FilePenLine,
  FolderSearch,
  GitMerge,
  RotateCcw,
  Trash2,
} from "lucide-vue-next";

import type { AutostartItem } from "@/api";
import { canEditScript, canRevealItem } from "@/modules/components/startup/script";
import { useI18n } from "@/modules/locales";

const props = withDefaults(
  defineProps<{
    open: boolean;
    x: number;
    y: number;
    item: AutostartItem | null;
    busy?: boolean;
  }>(),
  { busy: false },
);

const emit = defineEmits<{
  close: [];
  disable: [item: AutostartItem];
  restore: [item: AutostartItem];
  takeOver: [item: AutostartItem];
  reveal: [item: AutostartItem];
  edit: [item: AutostartItem];
  delete: [item: AutostartItem];
}>();

const { t } = useI18n();
const route = useRoute();

const EDGE = 8;
const menuEl = ref<HTMLElement | null>(null);
const left = ref(0);
const top = ref(0);

const item = computed(() => props.item);

/** 禁用：系统启用中且不是自身。 */
const disableDisabled = computed(
  () => props.busy || !item.value?.enabled || item.value?.isSelf === true,
);
/** 恢复：当前已停用或已被接管。 */
const restoreDisabled = computed(() => {
  const current = item.value;
  if (!current) return true;
  return (
    props.busy ||
    current.isSelf ||
    !(current.enabled === false || current.takenOver)
  );
});
/** 接管此项：未接管且不是自身（已移除行会出现在已接管状态，自动隐藏）。 */
const takeOverDisabled = computed(
  () =>
    props.busy ||
    !item.value ||
    item.value.takenOver ||
    item.value.isSelf === true ||
    item.value.removed === true,
);

/** 删除：后端声明可删除且不是自身。 */
const deleteDisabled = computed(
  () => props.busy || !item.value?.canDelete || item.value?.isSelf === true,
);

/** 在资源管理器中显示：有可执行文件路径或属于启动文件夹项。 */
const revealDisabled = computed(
  () => props.busy || !canRevealItem(item.value),
);

/** 编辑启动内容：扩展名命中白名单的文本脚本。 */
const editDisabled = computed(() => props.busy || !canEditScript(item.value));

const disableTitle = computed(() =>
  item.value?.isSelf ? t("startup.selfItem") : t("startup.menu.disable"),
);

const restoreTitle = computed(() =>
  item.value?.isSelf ? t("startup.selfItem") : t("startup.menu.restore"),
);

const revealTitle = computed(() => {
  const current = item.value;
  if (!current) return "";
  if (current.removed) return t("startup.removedDetail");
  if (!canRevealItem(current)) return "该启动项没有可定位的文件";
  return t("startup.menu.reveal");
});

const editTitle = computed(() => {
  const current = item.value;
  if (!current) return "";
  if (current.removed) return t("startup.removedDetail");
  if (!canEditScript(current)) return t("startup.editNotEditable");
  return t("startup.menu.editScript");
});

const deleteTitle = computed(() => {
  if (item.value?.isSelf) return t("startup.selfItem");
  if (!item.value?.canDelete) return t("startup.deleteUnsupported");
  return t("startup.menu.delete");
});

async function place() {
  const el = menuEl.value;
  const width = el?.offsetWidth ?? 200;
  const height = el?.offsetHeight ?? 140;
  const maxX = window.innerWidth - width - EDGE;
  const maxY = window.innerHeight - height - EDGE;
  left.value = Math.max(EDGE, Math.min(props.x, maxX));
  top.value = Math.max(EDGE, Math.min(props.y, maxY));
}

function focusFirstEnabled() {
  const button = menuEl.value?.querySelector<HTMLButtonElement>(
    "button:not([disabled])",
  );
  button?.focus({ preventScroll: true });
}

async function refresh() {
  if (!props.open) return;
  left.value = props.x;
  top.value = props.y;
  await nextTick();
  await place();
  focusFirstEnabled();
}

let bound = false;

function onPointerDown(event: MouseEvent) {
  if (menuEl.value && event.target instanceof Node) {
    if (menuEl.value.contains(event.target)) return;
  }
  close();
}

function onKeydown(event: KeyboardEvent) {
  if (event.key === "Escape") {
    close();
  }
}

function onScroll() {
  close();
}

function onBlur() {
  close();
}

function bind() {
  if (bound) return;
  bound = true;
  window.addEventListener("pointerdown", onPointerDown, true);
  window.addEventListener("keydown", onKeydown);
  window.addEventListener("scroll", onScroll, true);
  window.addEventListener("resize", onScroll);
  window.addEventListener("blur", onBlur);
}

function unbind() {
  if (!bound) return;
  bound = false;
  window.removeEventListener("pointerdown", onPointerDown, true);
  window.removeEventListener("keydown", onKeydown);
  window.removeEventListener("scroll", onScroll, true);
  window.removeEventListener("resize", onScroll);
  window.removeEventListener("blur", onBlur);
}

function close() {
  emit("close");
}

watch(
  () => props.open,
  (open) => {
    if (open) {
      bind();
      void refresh();
    } else {
      unbind();
    }
  },
);

watch(
  [() => props.x, () => props.y, () => props.item?.id],
  () => {
    if (props.open) void refresh();
  },
);

// 路由切换（含托盘导航）时关闭。
watch(
  () => route.fullPath,
  () => {
    if (props.open) close();
  },
);

onBeforeUnmount(unbind);
</script>

<template>
  <Teleport to="body">
    <Transition
      enter-active-class="transition duration-100 ease-out"
      enter-from-class="opacity-0 scale-95"
      enter-to-class="opacity-100 scale-100"
      leave-active-class="transition duration-75 ease-in"
      leave-from-class="opacity-100 scale-100"
      leave-to-class="opacity-0 scale-95"
    >
      <div
        v-if="open && item"
        ref="menuEl"
        class="fixed z-[70] w-52 origin-top-left overflow-hidden rounded-xl border border-zinc-200 bg-white py-1 shadow-xl shadow-zinc-900/10 dark:border-zinc-700 dark:bg-zinc-900 dark:shadow-black/40"
        :style="{ left: `${left}px`, top: `${top}px` }"
        role="menu"
        :aria-label="item.name"
        @contextmenu.prevent
      >
        <div class="border-b border-zinc-200/80 px-3 py-1.5 dark:border-zinc-800">
          <p
            class="truncate text-[11px] font-semibold text-zinc-500 dark:text-zinc-400"
            :title="item.name"
          >
            {{ item.name }}
          </p>
        </div>

        <div class="p-1">
          <button
            type="button"
            role="menuitem"
            class="flex w-full items-center gap-2 rounded-lg px-2.5 py-1.5 text-left text-xs text-zinc-700 outline-none transition hover:bg-zinc-100 focus-visible:bg-zinc-100 focus-visible:ring-2 focus-visible:ring-brand-500/50 disabled:cursor-not-allowed disabled:opacity-40 disabled:hover:bg-transparent dark:text-zinc-200 dark:hover:bg-zinc-800 dark:focus-visible:bg-zinc-800 dark:disabled:hover:bg-transparent"
            :disabled="disableDisabled"
            :title="disableTitle"
            @click="emit('disable', item)"
          >
            <Ban class="h-3.5 w-3.5 shrink-0 text-zinc-400 dark:text-zinc-500" />
            <span class="truncate">{{ t("startup.menu.disable") }}</span>
          </button>

          <button
            type="button"
            role="menuitem"
            class="flex w-full items-center gap-2 rounded-lg px-2.5 py-1.5 text-left text-xs text-zinc-700 outline-none transition hover:bg-zinc-100 focus-visible:bg-zinc-100 focus-visible:ring-2 focus-visible:ring-brand-500/50 disabled:cursor-not-allowed disabled:opacity-40 disabled:hover:bg-transparent dark:text-zinc-200 dark:hover:bg-zinc-800 dark:focus-visible:bg-zinc-800 dark:disabled:hover:bg-transparent"
            :disabled="restoreDisabled"
            :title="restoreTitle"
            @click="emit('restore', item)"
          >
            <RotateCcw
              class="h-3.5 w-3.5 shrink-0 text-zinc-400 dark:text-zinc-500"
            />
            <span class="truncate">{{ t("startup.menu.restore") }}</span>
          </button>

          <button
            v-if="!item.takenOver && !item.isSelf"
            type="button"
            role="menuitem"
            class="flex w-full items-center gap-2 rounded-lg px-2.5 py-1.5 text-left text-xs text-zinc-700 outline-none transition hover:bg-zinc-100 focus-visible:bg-zinc-100 focus-visible:ring-2 focus-visible:ring-brand-500/50 disabled:cursor-not-allowed disabled:opacity-40 disabled:hover:bg-transparent dark:text-zinc-200 dark:hover:bg-zinc-800 dark:focus-visible:bg-zinc-800 dark:disabled:hover:bg-transparent"
            :disabled="takeOverDisabled"
            :title="t('startup.menu.takeOver')"
            @click="emit('takeOver', item)"
          >
            <GitMerge
              class="h-3.5 w-3.5 shrink-0 text-zinc-400 dark:text-zinc-500"
            />
            <span class="truncate">{{ t("startup.menu.takeOver") }}</span>
          </button>
        </div>

        <div class="my-1 h-px bg-zinc-200/80 dark:bg-zinc-800" />

        <div class="p-1">
          <button
            type="button"
            role="menuitem"
            class="flex w-full items-center gap-2 rounded-lg px-2.5 py-1.5 text-left text-xs text-zinc-700 outline-none transition hover:bg-zinc-100 focus-visible:bg-zinc-100 focus-visible:ring-2 focus-visible:ring-brand-500/50 disabled:cursor-not-allowed disabled:opacity-40 disabled:hover:bg-transparent dark:text-zinc-200 dark:hover:bg-zinc-800 dark:focus-visible:bg-zinc-800 dark:disabled:hover:bg-transparent"
            :disabled="revealDisabled"
            :title="revealTitle"
            @click="emit('reveal', item)"
          >
            <FolderSearch
              class="h-3.5 w-3.5 shrink-0 text-zinc-400 dark:text-zinc-500"
            />
            <span class="truncate">{{ t("startup.menu.reveal") }}</span>
          </button>

          <button
            type="button"
            role="menuitem"
            class="flex w-full items-center gap-2 rounded-lg px-2.5 py-1.5 text-left text-xs text-zinc-700 outline-none transition hover:bg-zinc-100 focus-visible:bg-zinc-100 focus-visible:ring-2 focus-visible:ring-brand-500/50 disabled:cursor-not-allowed disabled:opacity-40 disabled:hover:bg-transparent dark:text-zinc-200 dark:hover:bg-zinc-800 dark:focus-visible:bg-zinc-800 dark:disabled:hover:bg-transparent"
            :disabled="editDisabled"
            :title="editTitle"
            @click="emit('edit', item)"
          >
            <FilePenLine
              class="h-3.5 w-3.5 shrink-0 text-zinc-400 dark:text-zinc-500"
            />
            <span class="truncate">{{ t("startup.menu.editScript") }}</span>
          </button>
        </div>

        <div class="my-1 h-px bg-zinc-200/80 dark:bg-zinc-800" />

        <div class="p-1 pt-0">
          <button
            type="button"
            role="menuitem"
            class="flex w-full items-center gap-2 rounded-lg px-2.5 py-1.5 text-left text-xs text-red-600 outline-none transition hover:bg-red-50 focus-visible:bg-red-50 focus-visible:ring-2 focus-visible:ring-red-500/40 disabled:cursor-not-allowed disabled:opacity-40 disabled:hover:bg-transparent dark:text-red-400 dark:hover:bg-red-500/10 dark:focus-visible:bg-red-500/10 dark:disabled:hover:bg-transparent"
            :disabled="deleteDisabled"
            :title="deleteTitle"
            @click="emit('delete', item)"
          >
            <Trash2 class="h-3.5 w-3.5 shrink-0" />
            <span class="truncate">{{ t("startup.menu.delete") }}</span>
          </button>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>
