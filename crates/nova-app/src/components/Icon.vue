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
    | "wand"
    | "copy"
    | "play"
    | "history"
    | "transfer"
    | "server"
    | "sun"
    | "moon"
    | "monitor"
    | "sidebar"
    | "cookie"
    | "eye"
    | "eye-off"
    | "lock"
    | "git-branch"
    | "upload"
    | "download"
    | "sparkle"
    | "shield"
    | "users"
    | "code"
    | "help-circle";
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

    <template v-else-if="name === 'copy'">
      <path d="M9 9V5a1 1 0 0 1 1-1h9a1 1 0 0 1 1 1v9a1 1 0 0 1-1 1h-4" />
      <path d="M5 9h9a1 1 0 0 1 1 1v9a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1v-9a1 1 0 0 1 1-1z" />
    </template>

    <path v-else-if="name === 'play'" d="M7 4.5v15l13-7.5z" />

    <template v-else-if="name === 'history'">
      <path d="M3 12a9 9 0 1 0 3-6.7" />
      <path d="M3 4v4.5h4.5" />
      <path d="M12 8v4l3 2" />
    </template>

    <template v-else-if="name === 'transfer'">
      <path d="M8 20V4M8 4L4.5 7.5M8 4l3.5 3.5" />
      <path d="M16 4v16M16 20l3.5-3.5M16 20l-3.5-3.5" />
    </template>

    <template v-else-if="name === 'server'">
      <rect x="3" y="4" width="18" height="6" rx="1.5" />
      <rect x="3" y="14" width="18" height="6" rx="1.5" />
      <path d="M7 7h.01M7 17h.01" />
    </template>

    <template v-else-if="name === 'sun'">
      <circle cx="12" cy="12" r="4" />
      <path
        d="M12 2v2.5M12 19.5V22M4.9 4.9l1.8 1.8M17.3 17.3l1.8 1.8M2 12h2.5M19.5 12H22M4.9 19.1l1.8-1.8M17.3 6.7l1.8-1.8"
      />
    </template>

    <path v-else-if="name === 'moon'" d="M20.5 14.5A8.5 8.5 0 1 1 9.5 3.5a7 7 0 0 0 11 11z" />

    <template v-else-if="name === 'monitor'">
      <rect x="2.5" y="4" width="19" height="13" rx="1.5" />
      <path d="M8 21h8M12 17v4" />
    </template>

    <template v-else-if="name === 'sidebar'">
      <rect x="3" y="4" width="18" height="16" rx="1.5" />
      <path d="M9.5 4v16" />
    </template>

    <template v-else-if="name === 'cookie'">
      <path
        d="M20.5 12.5A8.5 8.5 0 1 1 11.5 3.5c0 1.4 1.1 2.5 2.5 2.5s2.5 1.1 2.5 2.5 1.1 2.5 2.5 2.5 2 .6 2 1.5z"
      />
      <path d="M9 12a1 1 0 1 0 0-2 1 1 0 0 0 0 2zM13.5 17a1 1 0 1 0 0-2 1 1 0 0 0 0 2zM9.5 16.5h.01" />
    </template>

    <template v-else-if="name === 'eye'">
      <path d="M2.5 12S6 5.5 12 5.5 21.5 12 21.5 12 18 18.5 12 18.5 2.5 12 2.5 12z" />
      <circle cx="12" cy="12" r="2.75" />
    </template>

    <template v-else-if="name === 'eye-off'">
      <path
        d="M3.5 3.5l17 17M6.4 6.6C3.9 8.3 2.5 12 2.5 12S6 18.5 12 18.5c1.7 0 3.2-.5 4.4-1.1M10.6 10.6a2.75 2.75 0 0 0 3.9 3.9M9.9 5.6A9.4 9.4 0 0 1 12 5.5c6 0 9.5 6.5 9.5 6.5a15.5 15.5 0 0 1-2.7 3.5"
      />
    </template>

    <template v-else-if="name === 'lock'">
      <rect x="5" y="11" width="14" height="9" rx="1.5" />
      <path d="M8 11V7.5a4 4 0 0 1 8 0V11" />
    </template>

    <template v-else-if="name === 'git-branch'">
      <circle cx="6" cy="5" r="2.25" />
      <circle cx="6" cy="19" r="2.25" />
      <circle cx="18" cy="8" r="2.25" />
      <path d="M6 7.25V16.75" />
      <path d="M18 10.25V13a4 4 0 0 1-4 4h-2" />
    </template>

    <template v-else-if="name === 'upload'">
      <path d="M12 16V4M7.5 8.5L12 4l4.5 4.5" />
      <path d="M4 16.5V19a1 1 0 0 0 1 1h14a1 1 0 0 0 1-1v-2.5" />
    </template>

    <template v-else-if="name === 'download'">
      <path d="M12 4v12M7.5 11.5L12 16l4.5-4.5" />
      <path d="M4 16.5V19a1 1 0 0 0 1 1h14a1 1 0 0 0 1-1v-2.5" />
    </template>

    <path
      v-else-if="name === 'sparkle'"
      d="M12 2.5l2 6 6 2-6 2-2 6-2-6-6-2 6-2z"
      stroke-linejoin="round"
    />

    <template v-else-if="name === 'shield'">
      <path d="M12 3l7 3v5.5c0 4.5-3 7.5-7 9-4-1.5-7-4.5-7-9V6z" />
      <path d="M9 12l2 2 4-4" />
    </template>

    <template v-else-if="name === 'users'">
      <circle cx="9" cy="8" r="3" />
      <path d="M3 20c0-3.3 2.7-6 6-6s6 2.7 6 6" />
      <path d="M16 4.5a3 3 0 0 1 0 5.9" />
      <path d="M21 20c0-2.7-1.9-5-4.5-5.7" />
    </template>

    <template v-else-if="name === 'code'">
      <path d="M9 8L4 12.5 9 17" />
      <path d="M15 8l5 4.5-5 4.5" />
    </template>

    <template v-else-if="name === 'help-circle'">
      <circle cx="12" cy="12" r="9" />
      <path d="M9.3 9.3a2.7 2.7 0 1 1 3.9 2.4c-.9.5-1.2 1-1.2 2" />
      <path d="M12 17h.01" />
    </template>
  </svg>
</template>
