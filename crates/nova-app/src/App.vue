<script setup lang="ts">
import { ref } from "vue";

import { createRequest, openProject, pickProjectDirectory, validateProject } from "./api/nova";
import type { NovaProject, RequestFile } from "./types/nova";
import Sidebar from "./components/Sidebar.vue";
import ProjectPanel from "./components/ProjectPanel.vue";
import RequestPanel from "./components/RequestPanel.vue";
import EmptyState from "./components/EmptyState.vue";
import Modal from "./components/Modal.vue";

const project = ref<NovaProject | null>(null);
const validationIssues = ref<string[]>([]);
const selectedEnvironment = ref<string | null>(null);
const selectedRequest = ref<RequestFile | null>(null);
const error = ref<string | null>(null);
const createError = ref<string | null>(null);

// Tracks whether the currently-open RequestPanel/ProjectPanel has unsaved
// edits, so switching requests/projects can confirm before discarding them
// rather than silently losing an in-progress edit.
const requestPanelDirty = ref(false);
const projectPanelDirty = ref(false);

// In-app replacement for `window.confirm`: unimplemented (or flaky) in
// Tauri's webview on some platforms, and can't be styled to match the app.
const pendingDiscardConfirm = ref<((choice: boolean) => void) | null>(null);

function confirmDiscardIfDirty(): Promise<boolean> {
  if (!requestPanelDirty.value && !projectPanelDirty.value) return Promise.resolve(true);
  return new Promise((resolve) => {
    pendingDiscardConfirm.value = resolve;
  });
}

function resolveDiscardConfirm(choice: boolean) {
  pendingDiscardConfirm.value?.(choice);
  pendingDiscardConfirm.value = null;
}

async function handleOpen() {
  if (!(await confirmDiscardIfDirty())) return;

  const path = await pickProjectDirectory();
  if (!path) return;

  error.value = null;
  try {
    const [loaded, issues] = await Promise.all([openProject(path), validateProject(path)]);
    project.value = loaded;
    validationIssues.value = issues;
    selectedEnvironment.value =
      loaded.manifest.defaults.environment ?? loaded.environments[0]?.name ?? null;
    selectedRequest.value = null;
    requestPanelDirty.value = false;
    projectPanelDirty.value = false;
  } catch (e) {
    // Keep whatever project was already loaded (if any) so a failed
    // "switch project" attempt doesn't kick the user back to the empty
    // state and lose their current project.
    error.value = String(e);
  }
}

async function handleSelectRequest(request: RequestFile) {
  if (request.path === selectedRequest.value?.path) return;
  if (!(await confirmDiscardIfDirty())) return;
  selectedRequest.value = request;
  requestPanelDirty.value = false;
  // ProjectPanel (and any unsaved manifest edits in it) unmounts once a
  // request is selected, so its dirty flag no longer reflects anything on
  // screen.
  projectPanelDirty.value = false;
}

async function refreshProjectTree() {
  if (!project.value) return;
  try {
    project.value = await openProject(project.value.root);
  } catch (e) {
    error.value = String(e);
  }
}

// In-app replacement for `window.prompt` — same reasoning as the discard
// confirm above, plus a text-entry dialog has no equivalent in Tauri's
// dialog plugin at all (only message/confirm/file pickers).
const newRequestCollectionPath = ref<string | null>(null);
const newRequestName = ref("");

function handleCreateRequest(collectionPath: string) {
  createError.value = null;
  newRequestName.value = "";
  newRequestCollectionPath.value = collectionPath;
}

function cancelCreateRequest() {
  newRequestCollectionPath.value = null;
}

async function submitCreateRequest() {
  const collectionPath = newRequestCollectionPath.value;
  const name = newRequestName.value.trim();
  if (!collectionPath || !name) return;

  createError.value = null;
  try {
    const created = await createRequest(collectionPath, name);
    await refreshProjectTree();
    selectedRequest.value = created;
    requestPanelDirty.value = false;
    newRequestCollectionPath.value = null;
  } catch (e) {
    createError.value = String(e);
  }
}
</script>

<template>
  <div class="app-shell">
    <aside class="app-shell__sidebar">
      <Sidebar
        v-if="project"
        :project="project"
        v-model:selected-environment="selectedEnvironment"
        :selected-request-path="selectedRequest?.path"
        @select-request="handleSelectRequest"
        @switch-project="handleOpen"
        @create-request="handleCreateRequest"
      />
    </aside>

    <main class="app-shell__main">
      <p v-if="project && error" class="app-shell__error">{{ error }}</p>
      <RequestPanel
        v-if="project && selectedRequest"
        :key="selectedRequest.path"
        :request="selectedRequest"
        :selected-environment="selectedEnvironment"
        @dirty-change="requestPanelDirty = $event"
      />
      <ProjectPanel
        v-else-if="project"
        :project="project"
        :validation-issues="validationIssues"
        @dirty-change="projectPanelDirty = $event"
        @saved="refreshProjectTree"
      />
      <EmptyState v-else :error="error" @open="handleOpen" />
    </main>

    <Modal
      v-if="pendingDiscardConfirm"
      title="Discard unsaved changes?"
      @cancel="resolveDiscardConfirm(false)"
    >
      <p>This request has unsaved changes. Discard them?</p>
      <template #actions>
        <button type="button" class="button button--secondary" @click="resolveDiscardConfirm(false)">
          Cancel
        </button>
        <button type="button" class="button" @click="resolveDiscardConfirm(true)">Discard</button>
      </template>
    </Modal>

    <Modal
      v-if="newRequestCollectionPath !== null"
      title="New request"
      @cancel="cancelCreateRequest"
    >
      <label class="modal__label" for="new-request-name">Request name (e.g. get-users)</label>
      <input
        id="new-request-name"
        v-model="newRequestName"
        class="modal__input"
        type="text"
        autofocus
        @keydown.enter="submitCreateRequest"
      />
      <p v-if="createError" class="modal__error">{{ createError }}</p>
      <template #actions>
        <button type="button" class="button button--secondary" @click="cancelCreateRequest">
          Cancel
        </button>
        <button type="button" class="button" :disabled="!newRequestName.trim()" @click="submitCreateRequest">
          Create
        </button>
      </template>
    </Modal>
  </div>
</template>
