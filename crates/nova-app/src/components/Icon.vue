<script setup lang="ts">
// A small, fixed set of icons used throughout the app instead of unicode
// glyphs (✎, ×, +, 📁, ▾) — kept as one component with a closed `name`
// union rather than a per-icon file, so call sites (`<Icon name="trash" />`)
// never need to know or care where a glyph actually comes from.
//
// Most names render a `@heroicons/vue` (24px "solid") component — picked
// over hand-drawing a filled glyph for all ~40 names, since Heroicons
// already covers nearly every one of them well. A handful with no
// reasonable solid equivalent (a literal branching-diagram "git-branch", a
// "cookie", and a wand-with-sparkle "wand" distinct from the plain
// `sparkle` mark) keep the original hand-drawn stroke path here instead.
import { computed, type Component } from "vue";
import {
  ArrowDownTrayIcon,
  ArrowsRightLeftIcon,
  ArrowUpTrayIcon,
  ArrowsUpDownIcon,
  CheckIcon,
  ChevronDownIcon,
  CodeBracketIcon,
  Cog6ToothIcon,
  ComputerDesktopIcon,
  DocumentDuplicateIcon,
  DocumentPlusIcon,
  ExclamationTriangleIcon,
  EyeIcon,
  EyeSlashIcon,
  FolderIcon,
  FolderOpenIcon,
  FolderPlusIcon,
  LockClosedIcon,
  MoonIcon,
  PencilIcon,
  PlayIcon,
  PlusIcon,
  QuestionMarkCircleIcon,
  MagnifyingGlassIcon,
  ServerIcon,
  ShieldCheckIcon,
  SparklesIcon,
  SunIcon,
  TrashIcon,
  UsersIcon,
  ViewColumnsIcon,
  ClockIcon,
  XMarkIcon,
} from "@heroicons/vue/24/solid";

export type IconName =
  | "chevron-down"
  | "folder"
  | "folder-open"
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
  | "help-circle"
  | "search"
  | "warning";

const props = defineProps<{
  name: IconName;
}>();

const HEROICONS: Partial<Record<IconName, Component>> = {
  "chevron-down": ChevronDownIcon,
  folder: FolderIcon,
  "folder-open": FolderOpenIcon,
  "folder-plus": FolderPlusIcon,
  "file-plus": DocumentPlusIcon,
  pencil: PencilIcon,
  trash: TrashIcon,
  plus: PlusIcon,
  x: XMarkIcon,
  settings: Cog6ToothIcon,
  check: CheckIcon,
  swap: ArrowsRightLeftIcon,
  copy: DocumentDuplicateIcon,
  play: PlayIcon,
  history: ClockIcon,
  transfer: ArrowsUpDownIcon,
  server: ServerIcon,
  sun: SunIcon,
  moon: MoonIcon,
  monitor: ComputerDesktopIcon,
  sidebar: ViewColumnsIcon,
  eye: EyeIcon,
  "eye-off": EyeSlashIcon,
  lock: LockClosedIcon,
  upload: ArrowUpTrayIcon,
  download: ArrowDownTrayIcon,
  sparkle: SparklesIcon,
  shield: ShieldCheckIcon,
  users: UsersIcon,
  code: CodeBracketIcon,
  "help-circle": QuestionMarkCircleIcon,
  search: MagnifyingGlassIcon,
  warning: ExclamationTriangleIcon,
};

const heroicon = computed(() => HEROICONS[props.name]);
</script>

<template>
  <component
    :is="heroicon"
    v-if="heroicon"
    class="icon"
    :class="`icon--${props.name}`"
    aria-hidden="true"
  />
  <svg
    v-else
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
    <template v-if="name === 'wand'">
      <path d="M4 20L15 9" />
      <path d="M18 3l.8 1.7L20.5 5.5l-1.7.8-.8 1.7-.8-1.7-1.7-.8 1.7-.8z" />
      <path d="M13 6l1 1" />
    </template>

    <template v-else-if="name === 'git-branch'">
      <circle cx="6" cy="5" r="2.25" />
      <circle cx="6" cy="19" r="2.25" />
      <circle cx="18" cy="8" r="2.25" />
      <path d="M6 7.25V16.75" />
      <path d="M18 10.25V13a4 4 0 0 1-4 4h-2" />
    </template>

    <template v-else-if="name === 'cookie'">
      <path
        d="M20.5 12.5A8.5 8.5 0 1 1 11.5 3.5c0 1.4 1.1 2.5 2.5 2.5s2.5 1.1 2.5 2.5 1.1 2.5 2.5 2.5 2 .6 2 1.5z"
      />
      <path d="M9 12a1 1 0 1 0 0-2 1 1 0 0 0 0 2zM13.5 17a1 1 0 1 0 0-2 1 1 0 0 0 0 2zM9.5 16.5h.01" />
    </template>
  </svg>
</template>
