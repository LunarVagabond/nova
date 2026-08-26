<script setup lang="ts">
import { ref } from "vue";

import { createRequest, openProject, pickProjectDirectory, validateProject } from "./api/nova";
import type { NovaProject, RequestFile } from "./types/nova";
import Sidebar from "./components/Sidebar.vue";
import ProjectPanel from "./components/ProjectPanel.vue";
import RequestPanel from "./components/RequestPanel.vue";
import EmptyState from "./components/EmptyState.vue";

const project = ref<NovaProject | null>(null);
const validationIssues = ref<string[]>([]);
const selectedEnvironment = ref<string | null>(null);
const selectedRequest = ref<RequestFile | null>(null);
const error = ref<string | null>(null);
const createError = ref<string | null>(null);

// Tracks whether the currently-open RequestPanel has unsaved edits, so
// switching requests/projects can confirm before discarding them rather
// than silently losing an in-progress edit.
const requestPanelDirty = ref(false);

function confirmDiscardIfDirty(): boolean {
  if (!requestPanelDirty.value) return true;
  return window.confirm("You have unsaved changes to this request. Discard them?");
}

async function handleOpen() {
  if (!confirmDiscardIfDirty()) return;

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
  } catch (e) {
    // Keep whatever project was already loaded (if any) so a failed
    // "switch project" attempt doesn't kick the user back to the empty
    // state and lose their current project.
    error.value = String(e);
  }
}

function handleSelectRequest(request: RequestFile) {
  if (request.path === selectedRequest.value?.path) return;
  if (!confirmDiscardIfDirty()) return;
  selectedRequest.value = request;
  requestPanelDirty.value = false;
}

async function refreshProjectTree() {
  if (!project.value) return;
  try {
    project.value = await openProject(project.value.root);
  } catch (e) {
    error.value = String(e);
  }
}

async function handleCreateRequest(collectionPath: string) {
  const name = window.prompt("New request name (e.g. get-users):");
  if (!name) return;

  createError.value = null;
  try {
    const created = await createRequest(collectionPath, name);
    await refreshProjectTree();
    selectedRequest.value = created;
    requestPanelDirty.value = false;
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
      <p v-if="project && createError" class="app-shell__error">{{ createError }}</p>
      <RequestPanel
        v-if="project && selectedRequest"
        :key="selectedRequest.path"
        :request="selectedRequest"
        :selected-environment="selectedEnvironment"
        @dirty-change="requestPanelDirty = $event"
      />
      <ProjectPanel v-else-if="project" :project="project" :validation-issues="validationIssues" />
      <EmptyState v-else :error="error" @open="handleOpen" />
    </main>
  </div>
</template>
