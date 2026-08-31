<script setup lang="ts">
// The desktop app's Changes panel (#164) — commit/diff/push/pull/fetch for
// the git repository containing the open project, without leaving the
// app. A master/detail layout like `HistoryPanel.vue`: the changed-file
// list on the left (backed by the same `gitStatus` map the sidebar's tree
// badges use), a raw unified diff for whichever file is selected on the
// right.
//
// Staging is per-file (a checkbox toggles `gitStageFiles`/`gitUnstageFiles`
// for just that one path) plus a "Stage all" shortcut; the commit box
// itself is a `Modal.vue` dialog rather than inline in the panel, matching
// how every other "type a thing and confirm" action in this app works.
import { computed, ref, watch } from "vue";

import {
  gitCommitChanges,
  gitDiffFile,
  gitFetchRemote,
  gitPullRemote,
  gitPushRemote,
  gitStageFiles,
  gitStatus,
  gitUnstageFiles,
} from "../api/nova";
import { gitStatusKind, type GitFileStatus, type GitStatusMap } from "../types/nova";
import Icon from "./Icon.vue";
import Modal from "./Modal.vue";

const props = defineProps<{
  projectRoot: string;
  /** Only refetches while true — mirrors `HistoryPanel`'s `active` prop, so a hidden panel doesn't poll. */
  active: boolean;
}>();

const emit = defineEmits<{
  /** Stage/unstage/commit/pull all change the repository's git-visible state — lets the parent refresh the sidebar tree's own git badges. */
  changed: [];
}>();

const statusMap = ref<GitStatusMap | null>(null);
const loading = ref(false);
const loadError = ref<string | null>(null);

type FileEntry = { path: string; status: GitFileStatus };

const files = computed<FileEntry[]>(() => {
  if (!statusMap.value) return [];
  return Object.entries(statusMap.value)
    .map(([path, status]) => ({ path, status }))
    .sort((a, b) => a.path.localeCompare(b.path));
});

function fileName(path: string): string {
  return path.split("/").pop() ?? path;
}

function directoryOf(path: string): string {
  const parts = path.split("/");
  parts.pop();
  return parts.join("/");
}

const statusLabels: Record<ReturnType<typeof gitStatusKind>, string> = {
  untracked: "Untracked",
  unstaged: "Modified",
  staged: "Staged",
  committed: "Committed",
  renamed: "Renamed",
};

function statusLabel(status: GitFileStatus): string {
  return statusLabels[gitStatusKind(status)];
}

function isStaged(status: GitFileStatus): boolean {
  // `Renamed` covers both a git-detected staged rename and a nova-matched
  // unstaged one (see `nova_engine::GitFileStatus`) — there's no way to
  // tell those apart from the status alone, so a renamed row's checkbox
  // defaults to "staged" (the more common case: `git mv`).
  return gitStatusKind(status) === "staged" || gitStatusKind(status) === "renamed";
}

async function refresh() {
  loading.value = true;
  loadError.value = null;
  try {
    statusMap.value = await gitStatus(props.projectRoot);
  } catch (e) {
    loadError.value = String(e);
  } finally {
    loading.value = false;
  }
  if (selectedPath.value !== null) {
    if (statusMap.value?.[selectedPath.value]) {
      await loadDiff(selectedPath.value);
    } else {
      selectedPath.value = null;
      diffText.value = null;
    }
  }
}

watch(
  () => props.active,
  (active) => {
    if (active) refresh();
  },
  { immediate: true },
);

const selectedPath = ref<string | null>(null);
const diffText = ref<string | null>(null);
const diffLoading = ref(false);
const diffError = ref<string | null>(null);

async function loadDiff(path: string) {
  diffLoading.value = true;
  diffError.value = null;
  try {
    diffText.value = await gitDiffFile(props.projectRoot, path);
  } catch (e) {
    diffError.value = String(e);
  } finally {
    diffLoading.value = false;
  }
}

function selectFile(path: string) {
  selectedPath.value = path;
  loadDiff(path);
}

type DiffLine = { text: string; kind: "added" | "removed" | "hunk" | "context" };

const diffLines = computed<DiffLine[]>(() => {
  if (!diffText.value) return [];
  return diffText.value.split("\n").map((text) => {
    if (text.startsWith("@@")) return { text, kind: "hunk" as const };
    if (text.startsWith("+") && !text.startsWith("+++")) return { text, kind: "added" as const };
    if (text.startsWith("-") && !text.startsWith("---")) return { text, kind: "removed" as const };
    return { text, kind: "context" as const };
  });
});

const stageError = ref<string | null>(null);

async function toggleStage(entry: FileEntry) {
  stageError.value = null;
  try {
    if (isStaged(entry.status)) {
      await gitUnstageFiles(props.projectRoot, [entry.path]);
    } else {
      await gitStageFiles(props.projectRoot, [entry.path]);
    }
    await refresh();
    emit("changed");
  } catch (e) {
    stageError.value = String(e);
  }
}

async function stageAll() {
  stageError.value = null;
  try {
    await gitStageFiles(props.projectRoot, []);
    await refresh();
    emit("changed");
  } catch (e) {
    stageError.value = String(e);
  }
}

const stagedCount = computed(() => files.value.filter((f) => isStaged(f.status)).length);

// Commit dialog — a `Modal.vue`, matching the rest of the app rather than
// an inline box (see the file header comment).
const showCommitModal = ref(false);
const commitMessage = ref("");
const commitAmend = ref(false);
const commitBusy = ref(false);
const commitError = ref<string | null>(null);

function openCommitModal() {
  commitMessage.value = "";
  commitAmend.value = false;
  commitError.value = null;
  showCommitModal.value = true;
}

async function confirmCommit() {
  commitError.value = null;
  commitBusy.value = true;
  try {
    await gitCommitChanges(props.projectRoot, commitMessage.value, commitAmend.value);
    showCommitModal.value = false;
    await refresh();
    emit("changed");
  } catch (e) {
    commitError.value = String(e);
  } finally {
    commitBusy.value = false;
  }
}

// Fetch/pull/push — each shows git's own combined output (or error text)
// in a small strip under the header rather than trying to interpret it.
const remoteBusy = ref<"fetch" | "pull" | "push" | null>(null);
const remoteOutput = ref<string | null>(null);
const remoteError = ref<string | null>(null);

async function runRemoteAction(kind: "fetch" | "pull" | "push") {
  remoteBusy.value = kind;
  remoteOutput.value = null;
  remoteError.value = null;
  try {
    const action = kind === "fetch" ? gitFetchRemote : kind === "pull" ? gitPullRemote : gitPushRemote;
    remoteOutput.value = await action(props.projectRoot);
    if (kind === "pull") {
      await refresh();
      emit("changed");
    }
  } catch (e) {
    remoteError.value = String(e);
  } finally {
    remoteBusy.value = null;
  }
}
</script>

<template>
  <div class="changes-panel">
    <div class="changes-panel__list">
      <div class="changes-panel__list-header">
        <h2 class="changes-panel__title">Changes</h2>
        <div class="changes-panel__header-actions">
          <button type="button" class="icon-button icon-button--outline" title="Refresh" @click="refresh">
            <Icon name="history" />
          </button>
          <button
            type="button"
            class="icon-button icon-button--outline"
            title="Fetch"
            :disabled="remoteBusy !== null"
            @click="runRemoteAction('fetch')"
          >
            <Icon name="git-branch" />
          </button>
          <button
            type="button"
            class="icon-button icon-button--outline"
            title="Pull"
            :disabled="remoteBusy !== null"
            @click="runRemoteAction('pull')"
          >
            <Icon name="download" />
          </button>
          <button
            type="button"
            class="icon-button icon-button--outline"
            title="Push"
            :disabled="remoteBusy !== null"
            @click="runRemoteAction('push')"
          >
            <Icon name="upload" />
          </button>
        </div>
      </div>

      <p v-if="remoteBusy" class="changes-panel__hint">
        {{ remoteBusy === "fetch" ? "Fetching…" : remoteBusy === "pull" ? "Pulling…" : "Pushing…" }}
      </p>
      <p v-if="remoteError" class="changes-panel__error">{{ remoteError }}</p>
      <pre v-else-if="remoteOutput" class="changes-panel__remote-output">{{ remoteOutput || "(no output)" }}</pre>

      <p v-if="loading" class="changes-panel__hint">Loading…</p>
      <p v-else-if="loadError" class="changes-panel__error">{{ loadError }}</p>
      <p v-else-if="statusMap === null" class="changes-panel__hint">
        This project isn't inside a git repository.
      </p>
      <p v-else-if="files.length === 0" class="changes-panel__hint">Nothing changed — working tree is clean.</p>

      <template v-else>
        <div class="changes-panel__list-actions">
          <button type="button" class="button button--secondary" @click="stageAll">Stage all</button>
          <button type="button" class="button" :disabled="stagedCount === 0" @click="openCommitModal">
            Commit&hellip;
          </button>
        </div>
        <p v-if="stageError" class="changes-panel__error">{{ stageError }}</p>

        <ul class="changes-panel__entries">
          <li v-for="entry in files" :key="entry.path">
            <div class="changes-panel__entry" :class="{ 'changes-panel__entry--active': entry.path === selectedPath }">
              <input
                type="checkbox"
                class="changes-panel__checkbox"
                :checked="isStaged(entry.status)"
                :title="isStaged(entry.status) ? 'Unstage' : 'Stage'"
                @click.stop="toggleStage(entry)"
              />
              <button type="button" class="changes-panel__entry-button" @click="selectFile(entry.path)">
                <span
                  class="changes-panel__status-dot"
                  :class="`changes-panel__status-dot--${gitStatusKind(entry.status)}`"
                ></span>
                <span class="changes-panel__file-name" :title="entry.path">{{ fileName(entry.path) }}</span>
                <span class="changes-panel__file-dir">{{ directoryOf(entry.path) }}</span>
                <span class="changes-panel__status-label">{{ statusLabel(entry.status) }}</span>
              </button>
            </div>
          </li>
        </ul>
      </template>
    </div>

    <div class="changes-panel__diff">
      <p v-if="!selectedPath" class="changes-panel__hint">Select a changed file to view its diff.</p>
      <p v-else-if="diffLoading" class="changes-panel__hint">Loading…</p>
      <p v-else-if="diffError" class="changes-panel__error">{{ diffError }}</p>
      <p v-else-if="diffText === ''" class="changes-panel__hint">No differences to show for this file.</p>
      <div v-else class="changes-panel__diff-view">
        <div
          v-for="(line, index) in diffLines"
          :key="index"
          class="changes-panel__diff-line"
          :class="`changes-panel__diff-line--${line.kind}`"
        >{{ line.text }}</div>
      </div>
    </div>

    <Modal v-if="showCommitModal" title="Commit changes" @cancel="showCommitModal = false">
      <label class="changes-panel__field-label" for="changes-panel-commit-message">Message</label>
      <textarea
        id="changes-panel-commit-message"
        v-model="commitMessage"
        class="changes-panel__commit-message"
        rows="4"
        placeholder="Describe what changed"
        autofocus
      ></textarea>
      <label class="changes-panel__amend-label">
        <input v-model="commitAmend" type="checkbox" />
        Amend previous commit
      </label>
      <p v-if="commitError" class="modal__error">{{ commitError }}</p>
      <template #actions>
        <button type="button" class="button button--secondary" @click="showCommitModal = false">Cancel</button>
        <button
          type="button"
          class="button"
          :disabled="commitBusy || commitMessage.trim().length === 0"
          @click="confirmCommit"
        >
          {{ commitAmend ? "Amend" : "Commit" }}
        </button>
      </template>
    </Modal>
  </div>
</template>
