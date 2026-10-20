import { defineStore } from "pinia";
import { ref } from "vue";

export type Locale = "zh-CN" | "en";
export type ToastKind = "info" | "success" | "warn" | "error";

export interface Toast {
  id: number;
  kind: ToastKind;
  message: string;
}

const LOCALE_KEY = "daemon-man-locale";
const SIDEBAR_KEY = "daemon-man-sidebar-collapsed";

function readLocale(): Locale {
  const stored = localStorage.getItem(LOCALE_KEY);
  return stored === "en" ? "en" : "zh-CN";
}

export const useUiStore = defineStore("ui", () => {
  const locale = ref<Locale>(readLocale());
  const sidebarCollapsed = ref(localStorage.getItem(SIDEBAR_KEY) === "1");
  const toasts = ref<Toast[]>([]);
  let toastSeq = 0;

  function setLocale(next: Locale) {
    locale.value = next;
    localStorage.setItem(LOCALE_KEY, next);
    document.documentElement.lang = next;
  }

  function toggleSidebar() {
    sidebarCollapsed.value = !sidebarCollapsed.value;
    localStorage.setItem(SIDEBAR_KEY, sidebarCollapsed.value ? "1" : "0");
  }

  function notify(kind: ToastKind, message: string, duration = 3600) {
    const id = ++toastSeq;
    toasts.value.push({ id, kind, message });
    if (duration > 0) {
      window.setTimeout(() => dismiss(id), duration);
    }
    return id;
  }

  function dismiss(id: number) {
    toasts.value = toasts.value.filter((toast) => toast.id !== id);
  }

  return {
    locale,
    setLocale,
    sidebarCollapsed,
    toggleSidebar,
    toasts,
    notify,
    dismiss,
  };
});
