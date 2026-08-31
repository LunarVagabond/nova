<script setup lang="ts">
import { computed, onMounted, reactive, ref } from "vue";

import {
  createCollection,
  createEnvironment,
  createRequest,
  createWebSocketRequest,
  deleteCollection,
  deleteEnvironment,
  deleteRequest,
  duplicateRequest,
  exportProject,
  gitStatus as fetchGitStatus,
  importProject,
  initProject,
  mockServerStatus as fetchMockServerStatus,
  openProject,
  pickExportDestination,
  pickImportDestination,
  pickImportSource,
  pickProjectDirectory,
  renameCollection,
  renameRequest,
  runTests,
  startMockServer,
  stopMockServer,
  validateProject,
} from "./api/nova";
import type {
  Collection,
  GitStatusMap,
  ImportProjectOutcome,
  MockServerStatus,
  NovaEnvironment,
  NovaProject,
  RequestFile,
  TestRunResult,
} from "./types/nova";
import TopBar from "./components/TopBar.vue";
import Sidebar from "./components/Sidebar.vue";
import ProjectPanel from "./components/ProjectPanel.vue";
import RequestPanel from "./components/RequestPanel.vue";
import WebSocketPanel from "./components/WebSocketPanel.vue";
import EnvironmentPanel from "./components/EnvironmentPanel.vue";
import HistoryPanel from "./components/HistoryPanel.vue";
import CookiesPanel from "./components/CookiesPanel.vue";
import MockLogPanel from "./components/MockLogPanel.vue";
import EmptyState from "./components/EmptyState.vue";
import Modal from "./components/Modal.vue";
import Icon from "./components/Icon.vue";
import { useTheme, type ThemePreference } from "./composables/useTheme";
import { useResizablePane } from "./composables/useResizablePane";

const { preference: themePreference, setPreference: setThemePreference } = useTheme();

const MIN_MAIN_WIDTH = 360; // never let dragging squeeze the workspace below a usable width
const sidebarPane = useResizablePane({
  storageKey: "nova.sidebarWidth",
  lastExpandedStorageKey: "nova.sidebarLastExpandedWidth",
  defaultSize: 252,
  minSize: 32,
  getMax: (container) => (container ? container.clientWidth - MIN_MAIN_WIDTH - 7 : 600),
  axis: "horizontal",
  direction: 1, // dragging right grows the sidebar
});
const THEME_CYCLE: ThemePreference[] = ["system", "light", "dark"];
function cycleTheme() {
  const next = THEME_CYCLE[(THEME_CYCLE.indexOf(themePreference.value) + 1) % THEME_CYCLE.length];
  setThemePreference(next);
}

const SIDEBAR_HIDDEN_KEY = "nova.sidebarHidden";
const sidebarHidden = ref(localStorage.getItem(SIDEBAR_HIDDEN_KEY) === "true");
function toggleSidebarHidden() {
  sidebarHidden.value = !sidebarHidden.value;
  localStorage.setItem(SIDEBAR_HIDDEN_KEY, String(sidebarHidden.value));
}

const project = ref<NovaProject | null>(null);
const validationIssues = ref<string[]>([]);
const selectedEnvironment = ref<string | null>(null);

// Open request tabs — each keeps its own live `RequestPanel` instance (see
// the `v-show` block in the template) so switching tabs never discards an
// in-progress edit, only closing a dirty tab (or switching projects) does.
const openTabs = ref<RequestFile[]>([]);
const activeRequestPath = ref<string | null>(null);
const tabDirty = reactive<Record<string, boolean>>({});

// Per-file git status for the open project, keyed by absolute path —
// `null` when there's no project open or it isn't inside a git repo.
const gitStatus = ref<GitStatusMap | null>(null);

// The environment currently open for editing (variables/auth default) in
// the main panel — distinct from `selectedEnvironment` above, which is
// just which environment requests are sent against.
const managedEnvironment = ref<NovaEnvironment | null>(null);

// Bumped every time an environment's on-disk data changes (saved, created,
// or deleted) — RequestPanel's variables drawer watches this alongside the
// request path/selected environment so an edit made in EnvironmentPanel
// shows up there without the drawer having to be closed and reopened.
const environmentsVersion = ref(0);

// The desktop app's mock server toggle — its state lives in the Tauri
// backend (one server per app instance, independent of which project is
// open in the sidebar), so this mirrors it rather than owning it.
const mockServer = ref<MockServerStatus>({ running: false, host: null, port: null });
const mockServerBusy = ref(false);
const mockServerError = ref<string | null>(null);

// Which of the main-panel views is on screen. Explicit (rather than
// inferred from `managedEnvironment`/`openTabs.length`) so there's always a
// way back to "project" — e.g. a sidebar/header nav action — instead of it
// only being reachable as an implicit fallback.
type MainView = "request" | "environment" | "project" | "history" | "cookies" | "mock";
const mainView = ref<MainView>("project");

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

// Tracks whether the currently-open ProjectPanel/EnvironmentPanel (each a
// single remounted-on-switch instance, unlike the open request tabs above)
// has unsaved edits, so switching away can confirm before discarding them.
const projectPanelDirty = ref(false);
const environmentPanelDirty = ref(false);

// In-app replacement for `window.confirm`: unimplemented (or flaky) in
// Tauri's webview on some platforms, and can't be styled to match the app.
const pendingDiscardConfirm = ref<((choice: boolean) => void) | null>(null);

/** Resolves immediately when `isDirty` is false; otherwise waits on the discard-confirm modal. */
function confirmDiscard(isDirty: boolean): Promise<boolean> {
  if (!isDirty) return Promise.resolve(true);
  return new Promise((resolve) => {
    pendingDiscardConfirm.value = resolve;
  });
}

function resolveDiscardConfirm(choice: boolean) {
  pendingDiscardConfirm.value?.(choice);
  pendingDiscardConfirm.value = null;
}

async function handleOpen() {
  const anyTabDirty = Object.values(tabDirty).some(Boolean);
  if (!(await confirmDiscard(anyTabDirty || projectPanelDirty.value || environmentPanelDirty.value))) {
    return;
  }

  const path = await pickProjectDirectory();
  if (!path) return;

  await loadProject(path);
}

/** Refreshes the per-file git status badges for the currently open project. */
async function refreshGitStatus() {
  if (!project.value) {
    gitStatus.value = null;
    return;
  }
  try {
    gitStatus.value = await fetchGitStatus(project.value.root);
  } catch {
    // Git status is a supplementary indicator — never block on it.
    gitStatus.value = null;
  }
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
    openTabs.value = [];
    activeRequestPath.value = null;
    for (const key of Object.keys(tabDirty)) delete tabDirty[key];
    managedEnvironment.value = null;
    mainView.value = "project";
    projectPanelDirty.value = false;
    environmentPanelDirty.value = false;
    await refreshGitStatus();
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

/**
 * Opens `request` as a tab (or just activates it if already open). Also
 * used by the tab strip itself to switch the active tab, since re-opening
 * an already-open request is a no-op push.
 */
async function handleSelectRequest(request: RequestFile) {
  // The EnvironmentPanel/ProjectPanel view (and any unsaved edits in it)
  // unmounts once a request tab becomes the active view, so leaving either
  // one needs the same discard-confirm other view switches go through —
  // open tabs themselves are never discarded by this, only hidden.
  if (mainView.value === "environment") {
    if (!(await confirmDiscard(environmentPanelDirty.value))) return;
    managedEnvironment.value = null;
    environmentPanelDirty.value = false;
  } else if (mainView.value === "project") {
    if (!(await confirmDiscard(projectPanelDirty.value))) return;
    projectPanelDirty.value = false;
  }
  if (!openTabs.value.some((t) => t.path === request.path)) {
    openTabs.value.push(request);
  }
  activeRequestPath.value = request.path;
  mainView.value = "request";
}

/** Closes `request`'s tab, confirming first if it has unsaved edits. */
async function requestCloseTab(request: RequestFile) {
  if (!(await confirmDiscard(!!tabDirty[request.path]))) return;
  closeTabImmediate(request.path);
}

function closeTabImmediate(path: string) {
  const index = openTabs.value.findIndex((t) => t.path === path);
  if (index === -1) return;
  openTabs.value.splice(index, 1);
  delete tabDirty[path];
  if (activeRequestPath.value === path) {
    activeRequestPath.value = openTabs.value[openTabs.value.length - 1]?.path ?? null;
    // Nothing left to show as a request tab — fall back to the project
    // view rather than a blank main panel.
    if (activeRequestPath.value === null && mainView.value === "request") {
      mainView.value = "project";
    }
  }
}

/**
 * Switches to the project settings/manifest view — the sidebar's "Project
 * settings" action, and the only explicit way back to it once a request
 * tab or the environment editor is open (previously only reachable as a
 * fallback when nothing else was selected).
 */
async function showProjectSettings() {
  if (mainView.value === "project") return;
  if (mainView.value === "environment") {
    if (!(await confirmDiscard(environmentPanelDirty.value))) return;
    managedEnvironment.value = null;
    environmentPanelDirty.value = false;
  }
  mainView.value = "project";
}

/** Shows the current project's recent-sends history — a read-only view, so no dirty-discard check is needed to leave it. */
async function showHistory() {
  if (mainView.value === "history") return;
  if (mainView.value === "environment") {
    if (!(await confirmDiscard(environmentPanelDirty.value))) return;
    managedEnvironment.value = null;
    environmentPanelDirty.value = false;
  }
  mainView.value = "history";
}

/** Shows the current project's session cookie jar — edits there save immediately, so no dirty-discard check is needed to leave it. */
async function showCookies() {
  if (mainView.value === "cookies") return;
  if (mainView.value === "environment") {
    if (!(await confirmDiscard(environmentPanelDirty.value))) return;
    managedEnvironment.value = null;
    environmentPanelDirty.value = false;
  }
  mainView.value = "cookies";
}

/** Shows the running mock server's call log — a read-only view, so no dirty-discard check is needed to leave it. */
async function showMockLog() {
  if (mainView.value === "mock") return;
  if (mainView.value === "environment") {
    if (!(await confirmDiscard(environmentPanelDirty.value))) return;
    managedEnvironment.value = null;
    environmentPanelDirty.value = false;
  }
  mainView.value = "mock";
}

/** Recursively looks for a request at `path` anywhere in `collection`'s tree. */
function findRequestPath(collection: Collection, path: string): boolean {
  if (collection.requests.some((r) => r.path === path)) return true;
  return collection.children.some((child) => findRequestPath(child, path));
}

/**
 * Recursively finds the chain of collection names from `collection` down to
 * the one containing `targetPath`'s request, or `null` if it's not in this
 * subtree. Doesn't include `collection`'s own name — the root collection
 * itself has no label (mirrors `CollectionNode.vue`'s `isRoot`).
 */
function collectionBreadcrumb(
  collection: Collection,
  targetPath: string,
  trail: string[] = [],
): string[] | null {
  if (collection.requests.some((r) => r.path === targetPath)) return trail;
  for (const child of collection.children) {
    const found = collectionBreadcrumb(child, targetPath, [...trail, child.name]);
    if (found) return found;
  }
  return null;
}

const activeBreadcrumb = computed(() => {
  if (!project.value || !activeRequestPath.value) return null;
  const chain = collectionBreadcrumb(project.value.collections, activeRequestPath.value);
  if (!chain) return null;
  const tab = openTabs.value.find((t) => t.path === activeRequestPath.value);
  return tab ? [...chain, tab.name] : chain;
});

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
    await refreshGitStatus();
  } catch (e) {
    error.value = String(e);
    return;
  }

  // The refreshed tree may no longer contain a given open tab's request if
  // its collection (or an ancestor) was just renamed or deleted out from
  // under it — close it rather than leaving a dangling reference the
  // request panel would fail to load.
  for (const tab of [...openTabs.value]) {
    if (!findRequestPath(project.value.collections, tab.path)) {
      closeTabImmediate(tab.path);
    }
  }

  // Same idea for the environment currently open in EnvironmentPanel: if
  // it was just deleted, drop the reference and fall back to the project
  // view; otherwise re-point at the freshly reloaded copy (its name may
  // have changed). Editing an environment no longer implicitly changes
  // which one requests are sent against — that's the top-bar env selector's
  // job alone — so this doesn't touch `selectedEnvironment`.
  if (managedEnvironment.value) {
    const reloaded = project.value.environments.find((e) => e.path === managedEnvironment.value?.path);
    managedEnvironment.value = reloaded ?? null;
    if (!reloaded) {
      environmentPanelDirty.value = false;
      if (mainView.value === "environment") mainView.value = "project";
    }
  }

  // If the environment requests are sent against was renamed elsewhere,
  // follow it by path so sending doesn't silently start failing with a
  // stale name — mirrors what the engine now does for `defaults.environment`.
  if (selectedEnvironment.value) {
    const stillExists = project.value.environments.some((e) => e.name === selectedEnvironment.value);
    if (!stillExists && project.value.environments.length > 0) {
      selectedEnvironment.value =
        project.value.manifest.defaults.environment ?? project.value.environments[0].name;
    }
  }
}

// In-app replacement for `window.prompt` — same reasoning as the discard
// confirm above, plus a text-entry dialog has no equivalent in Tauri's
// dialog plugin at all (only message/confirm/file pickers).
const newRequestCollectionPath = ref<string | null>(null);
const newRequestName = ref("");
const newRequestProtocol = ref<"http" | "websocket">("http");

function handleCreateRequest(collectionPath: string) {
  createError.value = null;
  newRequestName.value = "";
  newRequestProtocol.value = "http";
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
    const created =
      newRequestProtocol.value === "websocket"
        ? await createWebSocketRequest(collectionPath, name)
        : await createRequest(collectionPath, name);
    await refreshProjectTree();
    managedEnvironment.value = null;
    if (!openTabs.value.some((t) => t.path === created.path)) {
      openTabs.value.push(created);
    }
    activeRequestPath.value = created.path;
    mainView.value = "request";
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

// Rename request.
const renamingRequest = ref<RequestFile | null>(null);
const renameRequestName = ref("");
const renameRequestError = ref<string | null>(null);

function handleRenameRequest(request: RequestFile) {
  renameRequestError.value = null;
  renameRequestName.value = request.name;
  renamingRequest.value = request;
}

function cancelRenameRequest() {
  renamingRequest.value = null;
}

async function submitRenameRequest() {
  const request = renamingRequest.value;
  const newName = renameRequestName.value.trim();
  if (!request || !newName) return;

  renameRequestError.value = null;
  try {
    const renamed = await renameRequest(request.path, newName);
    // Follow an open tab to its new path rather than letting
    // `refreshProjectTree` close it as a stale reference.
    const tabIndex = openTabs.value.findIndex((t) => t.path === request.path);
    if (tabIndex !== -1) {
      openTabs.value[tabIndex] = renamed;
      if (request.path in tabDirty) {
        tabDirty[renamed.path] = tabDirty[request.path];
        delete tabDirty[request.path];
      }
      if (activeRequestPath.value === request.path) {
        activeRequestPath.value = renamed.path;
      }
    }
    await refreshProjectTree();
    renamingRequest.value = null;
  } catch (e) {
    renameRequestError.value = String(e);
  }
}

// Duplicate request — same in-app prompt pattern as new request, above,
// pre-filled with a "<name> copy" suggestion.
const duplicatingRequest = ref<RequestFile | null>(null);
const duplicateRequestName = ref("");
const duplicateRequestError = ref<string | null>(null);

function handleDuplicateRequest(request: RequestFile) {
  duplicateRequestError.value = null;
  duplicateRequestName.value = `${request.name} copy`;
  duplicatingRequest.value = request;
}

function cancelDuplicateRequest() {
  duplicatingRequest.value = null;
}

async function submitDuplicateRequest() {
  const request = duplicatingRequest.value;
  const name = duplicateRequestName.value.trim();
  if (!request || !name) return;

  duplicateRequestError.value = null;
  try {
    const created = await duplicateRequest(request.path, name);
    await refreshProjectTree();
    if (!openTabs.value.some((t) => t.path === created.path)) {
      openTabs.value.push(created);
    }
    activeRequestPath.value = created.path;
    mainView.value = "request";
    duplicatingRequest.value = null;
  } catch (e) {
    duplicateRequestError.value = String(e);
  }
}

// Delete request — destructive, so this always goes through a confirm step
// in the in-app Modal rather than acting immediately. An open tab for the
// deleted request is closed automatically by `refreshProjectTree`, which
// drops any open tab no longer found in the reloaded collection tree.
const deletingRequest = ref<RequestFile | null>(null);
const deleteRequestError = ref<string | null>(null);

function handleDeleteRequest(request: RequestFile) {
  deleteRequestError.value = null;
  deletingRequest.value = request;
}

function cancelDeleteRequest() {
  deletingRequest.value = null;
}

async function confirmDeleteRequest() {
  const request = deletingRequest.value;
  if (!request) return;

  deleteRequestError.value = null;
  try {
    await deleteRequest(request.path);
    await refreshProjectTree();
    deletingRequest.value = null;
  } catch (e) {
    deleteRequestError.value = String(e);
  }
}

// Open the environment editor for the named environment (from the
// sidebar's "edit" affordance), mirroring `handleSelectRequest` above.
async function handleManageEnvironment(name: string) {
  const environment = project.value?.environments.find((e) => e.name === name);
  if (!environment) return;
  if (environment.path === managedEnvironment.value?.path) return;
  const isDirty =
    mainView.value === "project"
      ? projectPanelDirty.value
      : mainView.value === "environment"
        ? environmentPanelDirty.value
        : false;
  if (!(await confirmDiscard(isDirty))) return;

  projectPanelDirty.value = false;
  managedEnvironment.value = environment;
  mainView.value = "environment";
  environmentPanelDirty.value = false;
}

async function handleEnvironmentSaved() {
  await refreshProjectTree();
  environmentsVersion.value++;
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
    environmentsVersion.value++;
    // Jump straight into editing the new environment, the same way
    // creating a request immediately opens it.
    projectPanelDirty.value = false;
    selectedEnvironment.value = created.name;
    managedEnvironment.value = project.value.environments.find((e) => e.path === created.path) ?? created;
    mainView.value = "environment";
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
    environmentsVersion.value++;
    if (selectedEnvironment.value === environment.name) {
      selectedEnvironment.value =
        project.value?.manifest.defaults.environment ?? project.value?.environments[0]?.name ?? null;
    }
    deletingEnvironment.value = null;
  } catch (e) {
    deleteEnvironmentError.value = String(e);
  }
}

// "Run Tests" — runs every request in the whole open project against the
// currently selected environment and shows the results in a Modal.
// Per-collection/per-request scoping (the `run_tests` command already
// supports it) is left as a follow-up; this action always runs the whole
// project.
const runningTests = ref(false);
const testRunResult = ref<TestRunResult | null>(null);
const testRunError = ref<string | null>(null);

async function handleRunTests() {
  if (!project.value || runningTests.value) return;

  runningTests.value = true;
  testRunError.value = null;
  testRunResult.value = null;
  try {
    testRunResult.value = await runTests(project.value.root, selectedEnvironment.value);
  } catch (e) {
    testRunError.value = String(e);
  } finally {
    runningTests.value = false;
  }
}

onMounted(async () => {
  try {
    mockServer.value = await fetchMockServerStatus();
  } catch {
    // The toggle just starts from "off" if the initial status fetch fails
    // — never worth blocking app startup over.
  }
});

function closeTestResults() {
  testRunResult.value = null;
  testRunError.value = null;
}

// Import/export — one Modal covering both directions (see `Modal.vue`),
// opened from the top bar. Each direction drives the native file/folder
// pickers itself rather than collecting paths through form fields, the
// same way `handleOpen` above does for opening a project.
const importExportOpen = ref(false);
const importing = ref(false);
const importResult = ref<ImportProjectOutcome | null>(null);
const importError = ref<string | null>(null);
const exporting = ref(false);
const exportedTo = ref<string | null>(null);
const exportError = ref<string | null>(null);

function openImportExport() {
  importResult.value = null;
  importError.value = null;
  exportedTo.value = null;
  exportError.value = null;
  importExportOpen.value = true;
}

function closeImportExport() {
  if (importing.value || exporting.value) return;
  importExportOpen.value = false;
}

/**
 * Generates a new Nova project from a picked OpenAPI spec or Postman
 * collection export, into a picked destination folder — the GUI
 * equivalent of `nova generate`. Leaves the currently open project alone;
 * "Open imported project" (in the template) is a separate, explicit step.
 */
async function handleImport() {
  if (importing.value) return;
  importResult.value = null;
  importError.value = null;

  const source = await pickImportSource();
  if (!source) return;
  const destination = await pickImportDestination();
  if (!destination) return;

  importing.value = true;
  try {
    importResult.value = await importProject(source, destination);
  } catch (e) {
    importError.value = String(e);
  } finally {
    importing.value = false;
  }
}

/** Opens the just-imported project and closes the import/export dialog. */
async function openImportedProject() {
  const result = importResult.value;
  if (!result) return;
  importExportOpen.value = false;
  await loadProject(result.project_root);
}

/**
 * Exports the currently open project's collections as an OpenAPI spec to a
 * picked destination file — the GUI equivalent of `nova export`.
 */
async function handleExport() {
  if (!project.value || exporting.value) return;
  exportedTo.value = null;
  exportError.value = null;

  const destination = await pickExportDestination();
  if (!destination) return;

  exporting.value = true;
  try {
    await exportProject(project.value.root, destination);
    exportedTo.value = destination;
  } catch (e) {
    exportError.value = String(e);
  } finally {
    exporting.value = false;
  }
}

/**
 * Toggles the desktop app's mock server: starts it for the currently open
 * project if it's off, stops it if it's on. Errors (e.g. the configured
 * port is already in use by something else) surface the same way other
 * top-level failures do, via `error`.
 */
async function handleToggleMockServer() {
  if (mockServerBusy.value) return;

  mockServerBusy.value = true;
  mockServerError.value = null;
  try {
    if (mockServer.value.running) {
      mockServer.value = await stopMockServer();
    } else if (project.value) {
      mockServer.value = await startMockServer(project.value.root);
    }
  } catch (e) {
    mockServerError.value = String(e);
    error.value = mockServerError.value;
  } finally {
    mockServerBusy.value = false;
  }
}
</script>

<template>
  <div
    class="app-shell"
    :class="{ 'app-shell--sidebar-hidden': sidebarHidden }"
    :style="{ '--sidebar-width': sidebarPane.size.value + 'px' }"
    :ref="(el) => (sidebarPane.containerEl.value = el as HTMLElement | null)"
  >
    <TopBar
      :project-name="project?.manifest.project.name ?? null"
      :environments="project?.environments ?? []"
      :selected-environment="selectedEnvironment"
      :showing-project-settings="mainView === 'project'"
      :showing-history="mainView === 'history'"
      :showing-cookies="mainView === 'cookies'"
      :showing-mock-log="mainView === 'mock'"
      :running-tests="runningTests"
      :mock-server-status="mockServer"
      :mock-server-busy="mockServerBusy"
      :sidebar-hidden="sidebarHidden"
      :theme-preference="themePreference"
      @update:selected-environment="selectedEnvironment = $event"
      @switch-project="handleOpen"
      @project-settings="showProjectSettings"
      @show-history="showHistory"
      @show-cookies="showCookies"
      @show-mock-log="showMockLog"
      @run-tests="handleRunTests"
      @import-export="openImportExport"
      @toggle-mock-server="handleToggleMockServer"
      @toggle-sidebar="toggleSidebarHidden"
      @cycle-theme="cycleTheme"
    />

    <aside v-show="!sidebarHidden" class="app-shell__sidebar">
      <Sidebar
        v-if="project"
        :project="project"
        :selected-environment="selectedEnvironment"
        :selected-request-path="activeRequestPath"
        :git-status="gitStatus"
        @select-request="handleSelectRequest"
        @create-request="handleCreateRequest"
        @create-collection="handleCreateCollection"
        @rename-collection="handleRenameCollection"
        @delete-collection="handleDeleteCollection"
        @rename-request="handleRenameRequest"
        @duplicate-request="handleDuplicateRequest"
        @delete-request="handleDeleteRequest"
        @create-environment="handleCreateEnvironment"
        @manage-environment="handleManageEnvironment"
      />
    </aside>

    <div
      v-show="!sidebarHidden"
      class="app-shell__sidebar-divider"
      title="Drag to resize sidebar"
      @mousedown="sidebarPane.startDrag"
    ></div>

    <main class="app-shell__main">
      <div class="app-shell__chrome">
        <p v-if="project && error" class="app-shell__error">{{ error }}</p>

        <div v-if="project && openTabs.length > 0" class="tab-strip">
          <div class="tab-strip__tabs">
            <button
              v-for="tab in openTabs"
              :key="tab.path"
              type="button"
              class="tab-strip__tab"
              :class="{ 'tab-strip__tab--active': mainView === 'request' && tab.path === activeRequestPath }"
              @click="handleSelectRequest(tab)"
            >
              <span class="tab-strip__name">{{ tab.name }}</span>
              <span v-if="tabDirty[tab.path]" class="request-panel__dirty-dot"></span>
              <span class="tab-strip__close" title="Close" @click.stop="requestCloseTab(tab)">
                <Icon name="x" />
              </span>
            </button>
          </div>
          <button
            type="button"
            class="tab-strip__new"
            title="New request"
            @click="handleCreateRequest(project.collections.path)"
          >
            <Icon name="plus" />
          </button>
        </div>

        <p v-if="mainView === 'request' && activeBreadcrumb" class="request-breadcrumb">
          {{ activeBreadcrumb.join(" / ") }}
        </p>
      </div>

      <div
        class="app-shell__content"
        :class="{
          'app-shell__content--flush':
            mainView === 'request' || mainView === 'history' || mainView === 'cookies' || mainView === 'mock',
        }"
      >
        <template v-for="tab in openTabs" :key="tab.path">
          <WebSocketPanel
            v-if="tab.protocol === 'websocket'"
            v-show="project && mainView === 'request' && tab.path === activeRequestPath"
            :active="project !== null && mainView === 'request' && tab.path === activeRequestPath"
            :request="tab"
            :selected-environment="selectedEnvironment"
            :project-root="project?.root ?? ''"
            @dirty-change="tabDirty[tab.path] = $event"
            @saved="refreshGitStatus"
          />
          <RequestPanel
            v-else
            v-show="project && mainView === 'request' && tab.path === activeRequestPath"
            :active="project !== null && mainView === 'request' && tab.path === activeRequestPath"
            :request="tab"
            :selected-environment="selectedEnvironment"
            :project-root="project?.root ?? ''"
            :environments-version="environmentsVersion"
            @dirty-change="tabDirty[tab.path] = $event"
            @saved="refreshGitStatus"
          />
        </template>

        <EnvironmentPanel
          v-if="project && mainView === 'environment' && managedEnvironment"
          :key="managedEnvironment.path"
          :environment="managedEnvironment"
          :project-root="project.root"
          @dirty-change="environmentPanelDirty = $event"
          @saved="handleEnvironmentSaved"
          @delete="handleDeleteEnvironment"
        />
        <ProjectPanel
          v-else-if="project && mainView === 'project'"
          :project="project"
          :validation-issues="validationIssues"
          @dirty-change="projectPanelDirty = $event"
          @saved="refreshProjectTree"
        />
        <HistoryPanel
          v-else-if="project && mainView === 'history'"
          :project-root="project.root"
          :active="mainView === 'history'"
        />
        <CookiesPanel
          v-else-if="project && mainView === 'cookies'"
          :project-root="project.root"
          :active="mainView === 'cookies'"
        />
        <MockLogPanel
          v-else-if="project && mainView === 'mock'"
          :mock-server-status="mockServer"
          :active="mainView === 'mock'"
        />
        <EmptyState v-else-if="!project" :error="error" @open="handleOpen" />
      </div>
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
      <p>You have unsaved changes. Discard them?</p>
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
      <label class="modal__label">Protocol</label>
      <div class="modal__radio-group">
        <label class="modal__radio">
          <input v-model="newRequestProtocol" type="radio" name="new-request-protocol" value="http" />
          HTTP
        </label>
        <label class="modal__radio">
          <input v-model="newRequestProtocol" type="radio" name="new-request-protocol" value="websocket" />
          WebSocket
        </label>
      </div>
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

    <Modal v-if="renamingRequest" title="Rename request" @cancel="cancelRenameRequest">
      <label class="modal__label" for="rename-request-name">Request name</label>
      <input
        id="rename-request-name"
        v-model="renameRequestName"
        class="modal__input"
        type="text"
        autofocus
        @keydown.enter="submitRenameRequest"
      />
      <p v-if="renameRequestError" class="modal__error">{{ renameRequestError }}</p>
      <template #actions>
        <button type="button" class="button button--secondary" @click="cancelRenameRequest">
          Cancel
        </button>
        <button
          type="button"
          class="button"
          :disabled="!renameRequestName.trim()"
          @click="submitRenameRequest"
        >
          Rename
        </button>
      </template>
    </Modal>

    <Modal v-if="duplicatingRequest" title="Duplicate request" @cancel="cancelDuplicateRequest">
      <label class="modal__label" for="duplicate-request-name">New request name</label>
      <input
        id="duplicate-request-name"
        v-model="duplicateRequestName"
        class="modal__input"
        type="text"
        autofocus
        @keydown.enter="submitDuplicateRequest"
      />
      <p v-if="duplicateRequestError" class="modal__error">{{ duplicateRequestError }}</p>
      <template #actions>
        <button type="button" class="button button--secondary" @click="cancelDuplicateRequest">
          Cancel
        </button>
        <button
          type="button"
          class="button"
          :disabled="!duplicateRequestName.trim()"
          @click="submitDuplicateRequest"
        >
          Duplicate
        </button>
      </template>
    </Modal>

    <Modal v-if="deletingRequest" title="Delete request?" @cancel="cancelDeleteRequest">
      <p>
        Delete <strong>{{ deletingRequest.name }}</strong>? This removes it from disk and cannot
        be undone.
      </p>
      <p v-if="deleteRequestError" class="modal__error">{{ deleteRequestError }}</p>
      <template #actions>
        <button type="button" class="button button--secondary" @click="cancelDeleteRequest">
          Cancel
        </button>
        <button type="button" class="button button--danger" @click="confirmDeleteRequest">
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

    <Modal v-if="testRunError" title="Run Tests failed" @cancel="closeTestResults">
      <p class="modal__error">{{ testRunError }}</p>
      <template #actions>
        <button type="button" class="button" @click="closeTestResults">Close</button>
      </template>
    </Modal>

    <Modal v-if="testRunResult" title="Test results" wide @cancel="closeTestResults">
      <p class="test-results__summary">
        <span class="test-results__count test-results__count--passed">
          {{ testRunResult.passed }} passed
        </span>
        <span class="test-results__count test-results__count--failed">
          {{ testRunResult.failed }} failed
        </span>
      </p>

      <div class="test-results__list">
        <div v-for="result in testRunResult.requests" :key="result.path" class="test-results__request">
          <div class="test-results__request-header">
            <span
              class="test-results__status"
              :class="
                result.error || result.outcomes.some((o) => !o.passed)
                  ? 'test-results__status--fail'
                  : 'test-results__status--pass'
              "
            >
              {{ result.error || result.outcomes.some((o) => !o.passed) ? "Fail" : "Pass" }}
            </span>
            <span class="test-results__method">{{ result.method }}</span>
            <span class="test-results__url" :title="result.url || result.path">
              {{ result.url || result.path }}
            </span>
          </div>

          <p v-if="result.error" class="test-results__error">{{ result.error }}</p>

          <ul v-else-if="result.outcomes.length > 0" class="test-results__assertions">
            <li
              v-for="(outcome, index) in result.outcomes"
              :key="index"
              class="test-results__assertion"
              :class="outcome.passed ? 'test-results__assertion--pass' : 'test-results__assertion--fail'"
            >
              {{ outcome.passed ? "PASS" : "FAIL" }} — {{ outcome.raw }}
              <p v-if="outcome.failure" class="test-results__assertion-detail">{{ outcome.failure }}</p>
            </li>
          </ul>
        </div>
      </div>

      <template #actions>
        <button type="button" class="button" @click="closeTestResults">Close</button>
      </template>
    </Modal>

    <Modal v-if="importExportOpen" title="Import / export" @cancel="closeImportExport">
      <div class="import-export__section">
        <h3 class="import-export__heading">Import</h3>
        <p class="import-export__hint">
          Generate a new Nova project from an OpenAPI 3.x spec or a Postman collection export.
        </p>
        <button type="button" class="button button--secondary" :disabled="importing" @click="handleImport">
          {{ importing ? "Importing…" : "Choose a file to import…" }}
        </button>
        <p v-if="importError" class="modal__error">{{ importError }}</p>
        <div v-if="importResult" class="import-export__result">
          <p>
            Generated <strong>{{ importResult.request_count }}</strong> request(s) into
            <code>{{ importResult.project_root }}</code>.
          </p>
          <p v-for="(warning, index) in importResult.warnings" :key="index" class="import-export__warning">
            {{ warning }}
          </p>
          <button type="button" class="button" @click="openImportedProject">Open imported project</button>
        </div>
      </div>

      <div class="import-export__section">
        <h3 class="import-export__heading">Export</h3>
        <p class="import-export__hint">
          Export {{ project?.manifest.project.name ?? "this project" }}'s collections as an OpenAPI
          3.x spec (YAML).
        </p>
        <button
          type="button"
          class="button button--secondary"
          :disabled="!project || exporting"
          @click="handleExport"
        >
          {{ exporting ? "Exporting…" : "Choose where to save…" }}
        </button>
        <p v-if="exportError" class="modal__error">{{ exportError }}</p>
        <p v-if="exportedTo" class="import-export__result">
          Exported to <code>{{ exportedTo }}</code>.
        </p>
      </div>

      <template #actions>
        <button type="button" class="button button--secondary" :disabled="importing || exporting" @click="closeImportExport">
          Close
        </button>
      </template>
    </Modal>
  </div>
</template>
