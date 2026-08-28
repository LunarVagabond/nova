import { ref } from "vue";

export type ThemePreference = "system" | "light" | "dark";

const THEME_KEY = "nova.theme";

function isThemePreference(value: string | null): value is ThemePreference {
  return value === "system" || value === "light" || value === "dark";
}

function apply(pref: ThemePreference) {
  if (pref === "system") {
    delete document.documentElement.dataset.theme;
  } else {
    document.documentElement.dataset.theme = pref;
  }
}

/**
 * Call once, at the top level of a component's `<script setup>` (not
 * `onMounted`) so `data-theme` is set before first paint — `document.
 * documentElement` already exists by the time any component's setup runs,
 * since `main.ts` mounts onto it synchronously.
 */
export function useTheme() {
  const stored = localStorage.getItem(THEME_KEY);
  const preference = ref<ThemePreference>(isThemePreference(stored) ? stored : "system");

  apply(preference.value);

  function setPreference(pref: ThemePreference) {
    preference.value = pref;
    localStorage.setItem(THEME_KEY, pref);
    apply(pref);
  }

  return { preference, setPreference };
}
