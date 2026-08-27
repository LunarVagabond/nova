<script setup lang="ts">
// A small, fixed set of inline-SVG icons (stroke-based, 24x24, currentColor)
// used throughout the app instead of unicode glyphs (✎, ×, +, 📁, ▾) — kept
// as one component with a closed `name` union rather than a per-icon file
// or an icon-font dependency, since the set is small and deliberately not
// meant to grow casually.
const props = defineProps<{
  name:
    | "chevron-down"
    | "folder"
    | "folder-plus"
    | "file-plus"
    | "pencil"
    | "trash"
    | "plus"
    | "x"
    | "settings"
    | "check"
    | "swap"
    | "wand";
}>();
</script>

<template>
  <svg
    class="icon"
    :class="`icon--${props.name}`"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    stroke-width="2"
    stroke-linecap="round"
    stroke-linejoin="round"
    aria-hidden="true"
  >
    <path v-if="name === 'chevron-down'" d="M6 9l6 6 6-6" />

    <path
      v-else-if="name === 'folder'"
      d="M3 6.5a1 1 0 0 1 1-1h4.5l2 2H20a1 1 0 0 1 1 1V18a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1z"
    />

    <template v-else-if="name === 'folder-plus'">
      <path d="M3 6.5a1 1 0 0 1 1-1h4.5l2 2H20a1 1 0 0 1 1 1V18a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1z" />
      <path d="M12 11v5M9.5 13.5h5" />
    </template>

    <template v-else-if="name === 'file-plus'">
      <path d="M13 3H7a1 1 0 0 0-1 1v16a1 1 0 0 0 1 1h10a1 1 0 0 0 1-1V8z" />
      <path d="M13 3v5h5" />
      <path d="M9.5 15.5h5M12 13v5" />
    </template>

    <template v-else-if="name === 'pencil'">
      <path d="M12 20h9" />
      <path
        d="M16.5 3.5a2.12 2.12 0 0 1 3 3L7 19l-4 1 1-4z"
      />
    </template>

    <template v-else-if="name === 'trash'">
      <path d="M4 7h16" />
      <path d="M9 7V4.5a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1V7" />
      <path d="M6 7l1 13a1 1 0 0 0 1 1h8a1 1 0 0 0 1-1l1-13" />
    </template>

    <path v-else-if="name === 'plus'" d="M12 5v14M5 12h14" />

    <path v-else-if="name === 'x'" d="M18 6L6 18M6 6l12 12" />

    <template v-else-if="name === 'settings'">
      <circle cx="12" cy="12" r="3" />
      <path
        d="M19.4 13a7.97 7.97 0 0 0 0-2l2-1.5-2-3.4-2.3 1a8 8 0 0 0-1.8-1L15 3.6h-4l-.3 2.5a8 8 0 0 0-1.8 1l-2.3-1-2 3.4 2 1.5a7.97 7.97 0 0 0 0 2l-2 1.5 2 3.4 2.3-1a8 8 0 0 0 1.8 1l.3 2.5h4l.3-2.5a8 8 0 0 0 1.8-1l2.3 1 2-3.4z"
      />
    </template>

    <path v-else-if="name === 'check'" d="M5 13l4 4L19 7" />

    <template v-else-if="name === 'swap'">
      <path d="M4 8h14M14 8l3.5-3.5M14 8l3.5 3.5" />
      <path d="M20 16H6M10 16l-3.5-3.5M10 16l-3.5 3.5" />
    </template>

    <template v-else-if="name === 'wand'">
      <path d="M4 20L15 9" />
      <path d="M18 3l.8 1.7L20.5 5.5l-1.7.8-.8 1.7-.8-1.7-1.7-.8 1.7-.8z" />
      <path d="M13 6l1 1" />
    </template>
  </svg>
</template>
