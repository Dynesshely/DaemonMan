import { defineStore } from "pinia";
import { computed, ref } from "vue";

export type ThemeMode = "system" | "light" | "dark";

const THEME_KEY = "daemon-man-theme";

function readMode(): ThemeMode {
  const stored = localStorage.getItem(THEME_KEY);
  return stored === "light" || stored === "dark" ? stored : "system";
}

export const useThemeStore = defineStore("theme", () => {
  const mode = ref<ThemeMode>(readMode());
  const systemDark = ref(
    window.matchMedia("(prefers-color-scheme: dark)").matches,
  );

  const isDark = computed(
    () =>
      mode.value === "dark" || (mode.value === "system" && systemDark.value),
  );

  function apply() {
    document.documentElement.classList.toggle("dark", isDark.value);
  }

  function setMode(next: ThemeMode) {
    mode.value = next;
    localStorage.setItem(THEME_KEY, next);
    apply();
  }

  function toggle() {
    setMode(isDark.value ? "light" : "dark");
  }

  window
    .matchMedia("(prefers-color-scheme: dark)")
    .addEventListener("change", (event) => {
      systemDark.value = event.matches;
      apply();
    });

  apply();

  return { mode, isDark, setMode, toggle };
});
