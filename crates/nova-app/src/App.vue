<script setup lang="ts">
import { ref } from "vue";

import {
  createCollection,
  createEnvironment,
  createRequest,
  deleteCollection,
  deleteEnvironment,
  initProject,
  openProject,
  pickProjectDirectory,
  renameCollection,
  validateProject,
} from "./api/nova";
import type { Collection, NovaEnvironment, NovaProject, RequestFile } from "./types/nova";
import Sidebar from "./components/Sidebar.vue";
import ProjectPanel from "./components/ProjectPanel.vue";
import RequestPanel from "./components/RequestPanel.vue";
import EnvironmentPanel from "./components/EnvironmentPanel.vue";
import EmptyState from "./components/EmptyState.vue";
import Modal from "./components/Modal.vue";

const project = ref<NovaProject | null>(null);
const validationIssues = ref<string[]>([]);
const selectedEnvironment = ref<string | null>(null);
const selectedRequest = ref<RequestFile | null>(null);
// The environment currently open for editing (variables/auth default) in
// the main panel — distinct from `selectedEnvironment` above, which is
// just which environment requests are sent against.
const managedEnvironment = ref<NovaEnvironment | null>(null);
const error = ref<string | null>(null);
const createError = ref<string | null>(null);

// The directory a user picked that turned out to have no Nova project in
// it — non-null while the "open another / create one here" choice is up.
const notFoundPath = ref<string | null>(null);
// The directory the init form is filling in options for, once they've
// chosen to create a project. Kept separate from `notFoundPath` so the
// first dialog closes as the second one opens.
const initPath = ref<string | null>(null);
const initName = ref("");
const initInstallHook = ref(false);
const initError = ref<string | null>(null);
const initializing = ref(false);

// Tracks whether the currently-open RequestPanel/ProjectPanel/
// EnvironmentPanel has unsaved edits, so switching requests/projects can
// confirm before discarding them rather than silently losing an
// in-progress edit.
const requestPanelDirty = ref(false);
const projectPanelDirty = ref(false);
const environmentPanelDirty = ref(false);

// In-app replacement for `window.confirm`: unimplemented (or flaky) in
// Tauri's webview on some platforms, and can't be styled to match the app.
const pendingDiscardConfirm = ref<((choice: boolean) => void) | null>(null);

function confirmDiscardIfDirty(): Promise<boolean> {
  if (!requestPanelDirty.value && !projectPanelDirty.value && !environmentPanelDirty.value) {
    return Promise.resolve(true);
  }
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

  await loadProject(path);
}

/**
 * Opens the project at `path` and makes it the one on screen. A directory
 * with no project in it isn't an error — it opens the "there's nothing
 * here yet" choice instead, which can scaffold one on the spot.
 */
async function loadProject(path: string) {
  error.value = null;
  try {
    const outcome = await openProject(path);
    if (outcome === "not_found") {
      notFoundPath.value = path;
      return;
    }

    const loaded = outcome.found;
    // Only worth validating once we know there's something to validate.
    validationIssues.value = await validateProject(path);
    project.value = loaded;
    selectedEnvironment.value =
      loaded.manifest.defaults.environment ?? loaded.environments[0]?.name ?? null;
    selectedRequest.value = null;
    managedEnvironment.value = null;
    requestPanelDirty.value = false;
    projectPanelDirty.value = false;
    environmentPanelDirty.value = false;
  } catch (e) {
    // Keep whatever project was already loaded (if any) so a failed
    // "switch project" attempt doesn't kick the user back to the empty
    // state and lose their current project.
    error.value = String(e);
  }
}

function startInit() {
  initPath.value = notFoundPath.value;
  // Blank means "use the directory's own name", which the engine decides
  // — the same default `nova init` uses.
  initName.value = "";
  initInstallHook.value = false;
  initError.value = null;
  notFoundPath.value = null;
}

function cancelNotFound() {
  notFoundPath.value = null;
}

function cancelInit() {
  initPath.value = null;
  initError.value = null;
}

/** "Pick a different folder" — re-runs the folder picker from the choice. */
async function chooseAnotherFolder() {
  notFoundPath.value = null;
  const path = await pickProjectDirectory();
  if (!path) return;
  await loadProject(path);
}

async function submitInit() {
  const path = initPath.value;
  if (path === null || initializing.value) return;

  initializing.value = true;
  initError.value = null;
  try {
    const outcome = await initProject(path, {
      name: initName.value.trim() || null,
      installHook: initInstallHook.value,
    });
    // The project itself is on disk even if the (opt-in) hook failed, so
    // open it either way and surface the hook problem alongside it.
    const hookError =
      outcome.hook && "Err" in outcome.hook ? `Project created, but: ${outcome.hook.Err}` : null;
    initPath.value = null;
    await loadProject(outcome.project_root);
    if (hookError) error.value = hookError;
  } catch (e) {
    initError.value = String(e);
  } finally {
    initializing.value = false;
  }
}

async function handleSelectRequest(request: RequestFile) {
  if (request.path === selectedRequest.value?.path) return;
  if (!(await confirmDiscardIfDirty())) return;
  selectedRequest.value = request;
  managedEnvironment.value = null;
  requestPanelDirty.value = false;
  // ProjectPanel/EnvironmentPanel (and any unsaved edits in them) unmount
  // once a request is selected, so their dirty flags no longer reflect
  // anything on screen.
  projectPanelDirty.value = false;
  environmentPanelDirty.value = false;
}

/** Recursively looks for a request at `path` anywhere in `collection`'s tree. */
function findRequestPath(collection: Collection, path: string): boolean {
  if (collection.requests.some((r) => r.path === path)) return true;
  return collection.children.some((child) => findRequestPath(child, path));
}

async function refreshProjectTree() {
  if (!project.value) return;
  try {
    const outcome = await openProject(project.value.root);
    // This project was open a moment ago, so "not found" here means it
    // was moved or deleted out from under the app — an unexpected error,
    // not an invitation to scaffold a new one over it.
    if (outcome === "not_found") {
      error.value = `The project at ${project.value.root} is no longer there.`;
      return;
    }
    project.value = outcome.found;
  } catch (e) {
    error.value = String(e);
    return;
  }

  // The refreshed tree may no longer contain the currently-selected
  // request if its collection (or an ancestor) was just renamed or
  // deleted out from under it — clear the selection rather than leaving a
  // dangling reference the request panel would fail to load.
  if (selectedRequest.value && !findRequestPath(project.value.collections, selectedRequest.value.path)) {
    selectedRequest.value = null;
    requestPanelDirty.value = false;
  }

  // Same idea for the environment currently open in EnvironmentPanel: if
  // it was just deleted, drop the reference; otherwise re-point at the
  // freshly reloaded copy (its name may have changed) and keep the
  // environment picker's selection following it.
  if (managedEnvironment.value) {
    const reloaded = project.value.environments.find((e) => e.path === managedEnvironment.value?.path);
    managedEnvironment.value = reloaded ?? null;
    if (!reloaded) environmentPanelDirty.value = false;
    else if (selectedEnvironment.value !== reloaded.name) selectedEnvironment.value = reloaded.name;
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

// New collection: same in-app prompt pattern as new request, above.
const newCollectionParentPath = ref<string | null>(null);
const newCollectionName = ref("");
const newCollectionError = ref<string | null>(null);

function handleCreateCollection(parentPath: string) {
  newCollectionError.value = null;
  newCollectionName.value = "";
  newCollectionParentPath.value = parentPath;
}

function cancelCreateCollection() {
  newCollectionParentPath.value = null;
}

async function submitCreateCollection() {
  const parentPath = newCollectionParentPath.value;
  const name = newCollectionName.value.trim();
  if (!parentPath || !name) return;

  newCollectionError.value = null;
  try {
    await createCollection(parentPath, name);
    await refreshProjectTree();
    newCollectionParentPath.value = null;
  } catch (e) {
    newCollectionError.value = String(e);
  }
}

// Rename collection.
const renamingCollection = ref<Collection | null>(null);
const renameCollectionName = ref("");
const renameCollectionError = ref<string | null>(null);

function handleRenameCollection(collection: Collection) {
  renameCollectionError.value = null;
  renameCollectionName.value = collection.name;
  renamingCollection.value = collection;
}

function cancelRenameCollection() {
  renamingCollection.value = null;
}

async function submitRenameCollection() {
  const collection = renamingCollection.value;
  const newName = renameCollectionName.value.trim();
  if (!collection || !newName) return;

  renameCollectionError.value = null;
  try {
    await renameCollection(collection.path, newName);
    await refreshProjectTree();
    renamingCollection.value = null;
  } catch (e) {
    renameCollectionError.value = String(e);
  }
}

// Delete collection — destructive, so this always goes through a confirm
// step in the in-app Modal rather than acting immediately.
const deletingCollection = ref<Collection | null>(null);
const deleteCollectionError = ref<string | null>(null);

function handleDeleteCollection(collection: Collection) {
  deleteCollectionError.value = null;
  deletingCollection.value = collection;
}

function cancelDeleteCollection() {
  deletingCollection.value = null;
}

async function confirmDeleteCollection() {
  const collection = deletingCollection.value;
  if (!collection) return;

  deleteCollectionError.value = null;
  try {
    await deleteCollection(collection.path);
    await refreshProjectTree();
    deletingCollection.value = null;
  } catch (e) {
    deleteCollectionError.value = String(e);
  }
}

// Open the environment editor for the named environment (from the
// sidebar's "edit" affordance), mirroring `handleSelectRequest` above.
async function handleManageEnvironment(name: string) {
  const environment = project.value?.environments.find((e) => e.name === name);
  if (!environment) return;
  if (environment.path === managedEnvironment.value?.path) return;
  if (!(await confirmDiscardIfDirty())) return;

  selectedRequest.value = null;
  requestPanelDirty.value = false;
  projectPanelDirty.value = false;
  managedEnvironment.value = environment;
  environmentPanelDirty.value = false;
}

async function handleEnvironmentSaved() {
  await refreshProjectTree();
}

// New environment — same in-app prompt pattern as new request/collection.
const creatingEnvironment = ref(false);
const newEnvironmentName = ref("");
const newEnvironmentError = ref<string | null>(null);

function handleCreateEnvironment() {
  newEnvironmentError.value = null;
  newEnvironmentName.value = "";
  creatingEnvironment.value = true;
}

function cancelCreateEnvironment() {
  creatingEnvironment.value = false;
}

async function submitCreateEnvironment() {
  const name = newEnvironmentName.value.trim();
  if (!project.value || !name) return;

  newEnvironmentError.value = null;
  try {
    const created = await createEnvironment(project.value.environments_dir, name);
    await refreshProjectTree();
    // Jump straight into editing the new environment, the same way
    // creating a request immediately opens it.
    selectedRequest.value = null;
    requestPanelDirty.value = false;
    projectPanelDirty.value = false;
    selectedEnvironment.value = created.name;
    managedEnvironment.value = project.value.environments.find((e) => e.path === created.path) ?? created;
    environmentPanelDirty.value = false;
    creatingEnvironment.value = false;
  } catch (e) {
    newEnvironmentError.value = String(e);
  }
}

// Delete environment — destructive, so this always goes through a confirm
// step in the in-app Modal rather than acting immediately.
const deletingEnvironment = ref<NovaEnvironment | null>(null);
const deleteEnvironmentError = ref<string | null>(null);

function handleDeleteEnvironment(environment: NovaEnvironment) {
  deleteEnvironmentError.value = null;
  deletingEnvironment.value = environment;
}

function cancelDeleteEnvironment() {
  deletingEnvironment.value = null;
}

async function confirmDeleteEnvironment() {
  const environment = deletingEnvironment.value;
  if (!environment) return;

  deleteEnvironmentError.value = null;
  try {
    await deleteEnvironment(environment.path);
    await refreshProjectTree();
    if (selectedEnvironment.value === environment.name) {
      selectedEnvironment.value =
        project.value?.manifest.defaults.environment ?? project.value?.environments[0]?.name ?? null;
    }
    deletingEnvironment.value = null;
  } catch (e) {
    deleteEnvironmentError.value = String(e);
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
        @create-collection="handleCreateCollection"
        @rename-collection="handleRenameCollection"
        @delete-collection="handleDeleteCollection"
        @create-environment="handleCreateEnvironment"
        @manage-environment="handleManageEnvironment"
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
      <EnvironmentPanel
        v-else-if="project && managedEnvironment"
        :key="managedEnvironment.path"
        :environment="managedEnvironment"
        @dirty-change="environmentPanelDirty = $event"
        @saved="handleEnvironmentSaved"
        @delete="handleDeleteEnvironment"
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

    <Modal v-if="notFoundPath !== null" title="No Nova project here" @cancel="cancelNotFound">
      <p>
        <code>{{ notFoundPath }}</code> doesn't contain a
        <code>nova/nova.yaml</code> manifest, and neither does any directory above it.
      </p>
      <template #actions>
        <button type="button" class="button button--secondary" @click="cancelNotFound">
          Cancel
        </button>
        <button type="button" class="button button--secondary" @click="chooseAnotherFolder">
          Choose another folder
        </button>
        <button type="button" class="button" @click="startInit">Create a project here</button>
      </template>
    </Modal>

    <Modal v-if="initPath !== null" title="Create a project" @cancel="cancelInit">
      <p class="modal__hint">
        Creates <code>nova/</code> in <code>{{ initPath }}</code> with a starter environment,
        and adds <code>nova/envs/</code> to that directory's <code>.gitignore</code> — env
        files often hold secrets.
      </p>
      <label class="modal__label" for="init-name">Project name</label>
      <input
        id="init-name"
        v-model="initName"
        class="modal__input"
        type="text"
        placeholder="Defaults to the folder's name"
        autofocus
        @keydown.enter="submitInit"
      />
      <label class="modal__checkbox" for="init-install-hook">
        <input id="init-install-hook" v-model="initInstallHook" type="checkbox" />
        Install a git pre-commit hook that blocks commits containing a hardcoded credential
      </label>
      <p v-if="initError" class="modal__error">{{ initError }}</p>
      <template #actions>
        <button type="button" class="button button--secondary" @click="cancelInit">Cancel</button>
        <button type="button" class="button" :disabled="initializing" @click="submitInit">
          {{ initializing ? "Creating…" : "Create project" }}
        </button>
      </template>
    </Modal>

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

    <Modal
      v-if="newCollectionParentPath !== null"
      title="New collection"
      @cancel="cancelCreateCollection"
    >
      <label class="modal__label" for="new-collection-name">Collection name (e.g. users)</label>
      <input
        id="new-collection-name"
        v-model="newCollectionName"
        class="modal__input"
        type="text"
        autofocus
        @keydown.enter="submitCreateCollection"
      />
      <p v-if="newCollectionError" class="modal__error">{{ newCollectionError }}</p>
      <template #actions>
        <button type="button" class="button button--secondary" @click="cancelCreateCollection">
          Cancel
        </button>
        <button
          type="button"
          class="button"
          :disabled="!newCollectionName.trim()"
          @click="submitCreateCollection"
        >
          Create
        </button>
      </template>
    </Modal>

    <Modal v-if="renamingCollection" title="Rename collection" @cancel="cancelRenameCollection">
      <label class="modal__label" for="rename-collection-name">Collection name</label>
      <input
        id="rename-collection-name"
        v-model="renameCollectionName"
        class="modal__input"
        type="text"
        autofocus
        @keydown.enter="submitRenameCollection"
      />
      <p v-if="renameCollectionError" class="modal__error">{{ renameCollectionError }}</p>
      <template #actions>
        <button type="button" class="button button--secondary" @click="cancelRenameCollection">
          Cancel
        </button>
        <button
          type="button"
          class="button"
          :disabled="!renameCollectionName.trim()"
          @click="submitRenameCollection"
        >
          Rename
        </button>
      </template>
    </Modal>

    <Modal v-if="deletingCollection" title="Delete collection?" @cancel="cancelDeleteCollection">
      <p>
        Delete <strong>{{ deletingCollection.name }}</strong> and everything inside it? This
        removes its requests and any subcollections from disk and cannot be undone.
      </p>
      <p v-if="deleteCollectionError" class="modal__error">{{ deleteCollectionError }}</p>
      <template #actions>
        <button type="button" class="button button--secondary" @click="cancelDeleteCollection">
          Cancel
        </button>
        <button type="button" class="button button--danger" @click="confirmDeleteCollection">
          Delete
        </button>
      </template>
    </Modal>

    <Modal v-if="creatingEnvironment" title="New environment" @cancel="cancelCreateEnvironment">
      <label class="modal__label" for="new-environment-name">Environment name (e.g. staging)</label>
      <input
        id="new-environment-name"
        v-model="newEnvironmentName"
        class="modal__input"
        type="text"
        autofocus
        @keydown.enter="submitCreateEnvironment"
      />
      <p v-if="newEnvironmentError" class="modal__error">{{ newEnvironmentError }}</p>
      <template #actions>
        <button type="button" class="button button--secondary" @click="cancelCreateEnvironment">
          Cancel
        </button>
        <button
          type="button"
          class="button"
          :disabled="!newEnvironmentName.trim()"
          @click="submitCreateEnvironment"
        >
          Create
        </button>
      </template>
    </Modal>

    <Modal v-if="deletingEnvironment" title="Delete environment?" @cancel="cancelDeleteEnvironment">
      <p>
        Delete <strong>{{ deletingEnvironment.name }}</strong>? This removes its variables and
        default auth from disk and cannot be undone.
      </p>
      <p v-if="deleteEnvironmentError" class="modal__error">{{ deleteEnvironmentError }}</p>
      <template #actions>
        <button type="button" class="button button--secondary" @click="cancelDeleteEnvironment">
          Cancel
        </button>
        <button type="button" class="button button--danger" @click="confirmDeleteEnvironment">
          Delete
        </button>
      </template>
    </Modal>
  </div>
</template>
