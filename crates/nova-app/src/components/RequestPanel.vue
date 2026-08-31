<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, watch } from "vue";

import {
  diffAgainstExampleResponse,
  diffAgainstPreviousRun,
  exportRequestAs,
  getResolvedVariables,
  parseCurlCommand,
  parseGraphqlBody,
  parseMultipartBody,
  readRequest,
  saveRequest,
  saveResponseAsExample,
  sendRequest,
  serializeGraphqlBody,
  serializeMultipartBody,
} from "../api/nova";
import type {
  AuthScheme,
  ExportFormat,
  GraphQlBody,
  MultipartField,
  QueryParam,
  RequestDraft,
  RequestFile,
  RequestHeader,
  RequestResponse,
  ResolvedVariables,
  ResponseDiff,
} from "../types/nova";
import {
  BODY_TYPE_CONTENT_TYPES,
  BODY_TYPE_LABELS,
  BODY_TYPE_OPTIONS,
  detectBodyType,
  languageForContentType,
  parseBinaryBodyPath,
  randomBoundary,
  RAW_LANGUAGE_CONTENT_TYPES,
  RAW_LANGUAGE_LABELS,
  RAW_LANGUAGE_OPTIONS,
  serializeBinaryBodyPath,
  type BodyType,
  type RawLanguage,
} from "../lib/bodyType";
import { formatBytes, statusClass } from "../lib/format";
import { parseQueryString, serializeQuery, splitUrlAndQuery } from "../lib/queryString";
import { beautifyJson } from "../lib/jsonFormat";
import { formatXml } from "../lib/xmlFormat";
import AuthEditor from "./AuthEditor.vue";
import BinaryEditor from "./BinaryEditor.vue";
import CodeEditor, { type EditorLanguage } from "./CodeEditor.vue";
import Icon from "./Icon.vue";
import KeyValueEditor from "./KeyValueEditor.vue";
import MultipartEditor from "./MultipartEditor.vue";
import ResponseDiffView from "./ResponseDiffView.vue";
import { useResizablePane } from "../composables/useResizablePane";

const HTTP_METHODS = ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];

const props = defineProps<{
  request: RequestFile;
  selectedEnvironment: string | null;
  /** The open project's Nova root (`NovaProject.root`) — a multipart file attachment's path is stored relative to this. */
  projectRoot: string;
  active: boolean;
  /** Bumped by the parent whenever an environment is saved/created/deleted, so the variables drawer's watch below picks up an edit made in EnvironmentPanel without needing the request/environment selection itself to change. */
  environmentsVersion: number;
}>();

const emit = defineEmits<{
  (e: "dirtyChange", dirty: boolean): void;
  (e: "saved"): void;
}>();

const loading = ref(false);
const loadError = ref<string | null>(null);

const original = ref<RequestDraft | null>(null);

// Editable working copies — split out from `original` rather than editing
// it in place, so dirty-state is a plain comparison against the last
// loaded/saved snapshot.
const method = ref("GET");
const url = ref("");
const query = ref<QueryParam[]>([]);
const headers = ref<RequestHeader[]>([]);
const bodyText = ref("");
const auth = ref<AuthScheme | null>(null);
// `[settings]`' sync_content_type: whether picking a body type also
// rewrites the Content-Type header (see `handleBodyTypeChange`).
const syncContentType = ref(true);
// The `[assert]` section's assertion/extraction lines, edited as raw text
// (see `RequestDraft.assert_text`) rather than a structured rule builder —
// the engine's directive parser is the single source of truth for what's
// valid, surfaced here as a save-time error rather than duplicated
// client-side.
const assertText = ref("");
// The `[script]` section's `pre:`/`post:` script name or path, if any.
const scriptPre = ref("");
const scriptPost = ref("");

type FieldTab = "auth" | "headers" | "params" | "body" | "scripts" | "tests";
const activeTab = ref<FieldTab>("auth");

type ResponseTab = "headers" | "raw" | "preview" | "diff";
const activeResponseTab = ref<ResponseTab>("preview");

// Driven by the Content-Type header (and whether there's any body text at
// all) — set explicitly by `handleBodyTypeChange` below, and re-derived
// whenever a request is (re)loaded so it reflects what the file actually
// has, not just the last selection made in this session.
const bodyType = ref<BodyType>("none");
// Only meaningful while `bodyType` is "raw" — which sub-language's editor/
// Content-Type applies (Text/JavaScript/JSON/HTML/XML).
const rawLanguage = ref<RawLanguage>("text");

function upsertContentTypeHeader(value: string | null) {
  const index = headers.value.findIndex((h) => h.name.toLowerCase() === "content-type");
  if (value === null) {
    if (index !== -1) headers.value = headers.value.filter((_, i) => i !== index);
    return;
  }
  headers.value =
    index === -1
      ? [...headers.value, { name: "Content-Type", value }]
      : headers.value.map((h, i) => (i === index ? { ...h, value } : h));
}

// With syncing on (the default), selecting a body type is the source of
// truth for the Content-Type header: "No Body" clears both the text and
// the header entirely, and every other option sets the header to its
// canonical value (a fresh boundary for multipart) so the two never
// disagree.
//
// With syncing off, the selection is display-only: it still drives the
// editor's syntax highlighting, but the Content-Type header is left
// entirely to the Headers tab — which is the point of the setting, for a
// request that deliberately pairs, say, `application/vnd.acme+json` with a
// JSON-shaped body.
function handleBodyTypeChange(next: BodyType) {
  if (next === bodyType.value) return;
  bodyType.value = next;

  if (next === "multipart") {
    // Whatever text the body held under its previous type almost never
    // means anything as multipart parts — start the field table empty
    // rather than trying to reinterpret it.
    multipartFields.value = [];
    multipartParseError.value = null;
  } else if (next === "graphql") {
    graphqlQuery.value = "";
    graphqlVariablesText.value = "";
    graphqlVariablesError.value = null;
  } else if (next === "raw") {
    rawLanguage.value = "text";
  } else if (next === "binary") {
    // Whatever text the body held under its previous type has no meaning
    // as a file reference — start empty until a file is chosen.
    bodyText.value = "";
  }

  if (!syncContentType.value) return;

  if (next === "none") {
    bodyText.value = "";
    upsertContentTypeHeader(null);
    return;
  }

  const contentType =
    next === "multipart"
      ? `multipart/form-data; boundary=${randomBoundary()}`
      : next === "raw"
        ? RAW_LANGUAGE_CONTENT_TYPES[rawLanguage.value]
        : BODY_TYPE_CONTENT_TYPES[next];
  upsertContentTypeHeader(contentType);
}

/** The language sub-choice shown only while `bodyType` is "raw". */
function handleRawLanguageChange(next: RawLanguage) {
  rawLanguage.value = next;
  if (syncContentType.value) {
    upsertContentTypeHeader(RAW_LANGUAGE_CONTENT_TYPES[next]);
  }
}

// Structured view of the Body tab's `multipart` body type — mirrors
// `formFields` below, but the multipart wire format (boundaries,
// Content-Disposition/-Type/-Location per part) is parsed/serialized by
// the engine rather than reimplemented here, so it can't be a plain
// computed get/set: `parseMultipartBody`/`serializeMultipartBody` are
// async Tauri calls.
//
// `multipartFields` is therefore its own ref, kept in sync with `bodyText`
// in one direction at a time:
//  - `refreshMultipartFieldsFromText` (called on load, and after a curl
//    paste) parses `bodyText` into `multipartFields`.
//  - the watcher below serializes `multipartFields` back into `bodyText`
//    whenever the user edits a row.
// `applyingParsedFields` guards against the second reacting to the first's
// own assignment and immediately re-serializing (usually harmless, but
// pointlessly marks a freshly loaded request dirty before it's touched).
const multipartFields = ref<MultipartField[]>([]);
const multipartParseError = ref<string | null>(null);
let applyingParsedFields = false;

async function refreshMultipartFieldsFromText() {
  multipartParseError.value = null;
  try {
    const fields = await parseMultipartBody(headers.value, bodyText.value);
    applyingParsedFields = true;
    multipartFields.value = fields;
    await nextTick();
    applyingParsedFields = false;
  } catch (e) {
    multipartFields.value = [];
    multipartParseError.value = String(e);
  }
}

watch(
  multipartFields,
  async (fields) => {
    if (applyingParsedFields) return;
    try {
      bodyText.value = await serializeMultipartBody(fields, headers.value);
    } catch {
      // Nothing sensible to show the user here — the engine only rejects
      // this when the Content-Type header stopped naming a boundary out
      // from under the editor, which the Body tab's own controls don't
      // allow while `bodyType` is "multipart".
    }
  },
  { deep: true },
);

// Structured view of the Body tab's `graphql` body type — same
// two-refs-plus-watcher shape as `multipartFields` above, since
// `parseGraphqlBody`/`serializeGraphqlBody` are async Tauri calls too, not
// a plain computed get/set.
const graphqlQuery = ref("");
const graphqlVariablesText = ref("");
const graphqlVariablesError = ref<string | null>(null);
let applyingParsedGraphql = false;

async function refreshGraphqlFromText() {
  graphqlVariablesError.value = null;
  try {
    const parsed = await parseGraphqlBody(bodyText.value);
    applyingParsedGraphql = true;
    graphqlQuery.value = parsed.query;
    graphqlVariablesText.value = parsed.variables ? JSON.stringify(parsed.variables, null, 2) : "";
    await nextTick();
    applyingParsedGraphql = false;
  } catch (e) {
    graphqlQuery.value = "";
    graphqlVariablesText.value = "";
    graphqlVariablesError.value = String(e);
  }
}

watch([graphqlQuery, graphqlVariablesText], async ([query, variablesText]) => {
  if (applyingParsedGraphql) return;

  let variables: unknown = null;
  if (variablesText.trim() !== "") {
    try {
      variables = JSON.parse(variablesText);
    } catch {
      // Leave `bodyText` as it was — a half-typed JSON object is an
      // expected, transient state while editing, not worth erroring loudly
      // over on every keystroke; the field itself shows the message below.
      graphqlVariablesError.value = "Variables must be valid JSON";
      return;
    }
  }
  graphqlVariablesError.value = null;

  const draft: GraphQlBody = { query, variables, operation_name: null };
  try {
    bodyText.value = await serializeGraphqlBody(draft);
  } catch {
    // Mirrors the multipart watcher above — nothing sensible to show here.
  }
});

const saving = ref(false);
const saveError = ref<string | null>(null);

const sending = ref(false);
const sendError = ref<string | null>(null);
const response = ref<RequestResponse | null>(null);

// Drives the response pane's Diff tab (#90) — "vs Previous Run" compares
// the latest send against the one before it in this project's session
// history; "vs Saved Example" compares it against the request file's own
// hand-written `[response]` example, when it has one. Loaded on demand
// (see the watcher below) rather than alongside every send, since most
// sends never open this tab.
const diffMode = ref<"previous" | "example">("previous");
const diffResult = ref<ResponseDiff | null>(null);
const diffLoading = ref(false);
const diffError = ref<string | null>(null);
const hasExampleResponse = computed(() => original.value?.has_example_response ?? false);

async function loadDiff() {
  diffLoading.value = true;
  diffError.value = null;
  diffResult.value = null;
  try {
    diffResult.value =
      diffMode.value === "previous"
        ? await diffAgainstPreviousRun(props.request.path, props.selectedEnvironment)
        : await diffAgainstExampleResponse(props.request.path, props.selectedEnvironment);
  } catch (e) {
    diffError.value = String(e);
  } finally {
    diffLoading.value = false;
  }
}

watch([activeResponseTab, diffMode], ([tab]) => {
  if (tab === "diff") loadDiff();
});

const dirty = computed(() => {
  if (!original.value) return false;
  return (
    method.value !== original.value.method ||
    url.value !== original.value.url ||
    JSON.stringify(query.value) !== JSON.stringify(original.value.query) ||
    JSON.stringify(headers.value) !== JSON.stringify(original.value.headers) ||
    bodyText.value !== original.value.body_text ||
    JSON.stringify(auth.value) !== JSON.stringify(original.value.auth) ||
    syncContentType.value !== original.value.sync_content_type ||
    assertText.value !== original.value.assert_text ||
    (scriptPre.value.trim() === "" ? null : scriptPre.value) !== original.value.script_pre ||
    (scriptPost.value.trim() === "" ? null : scriptPost.value) !== original.value.script_post
  );
});

watch(dirty, (value) => emit("dirtyChange", value));

const editorLanguage = computed<EditorLanguage>(() => (bodyType.value === "raw" ? rawLanguage.value : "text"));

// Two-way sync between the URL field and the Params tab: `url` itself
// always stays the bare base URL (what actually gets saved as
// `[request].url`), and `query` is the single source of truth for
// parameters — the URL *input*'s displayed/edited text is a derived view
// that merges the two back together for display and splits them back apart
// on edit, so either surface can be used and the other stays in sync.
const urlDisplay = computed<string>({
  get() {
    const queryString = serializeQuery(query.value);
    return queryString ? `${url.value}?${queryString}` : url.value;
  },
  set(raw) {
    const split = splitUrlAndQuery(raw);
    url.value = split.base;
    query.value = split.query;
  },
});

// A structured view of the Body tab's `form` body type — reuses the same
// URLSearchParams-based encode/decode the URL bar's query-string sync
// already relies on, since `application/x-www-form-urlencoded` is exactly
// that same wire format applied to the body instead of the URL.
const formFields = computed<QueryParam[]>({
  get() {
    return parseQueryString(bodyText.value);
  },
  set(rows) {
    bodyText.value = serializeQuery(rows);
  },
});

// A structured view of the Body tab's `binary` body type — unlike
// multipart/GraphQL, the `@file: <path>` marker line (see
// `nova_engine::RequestBody::from_text`/`to_body_text`) is simple enough to
// parse/serialize with plain JS rather than an async engine round-trip, so
// this is a synchronous computed get/set like `formFields` above.
const binaryFilePath = computed<string | null>({
  get() {
    return parseBinaryBodyPath(bodyText.value);
  },
  set(filePath) {
    bodyText.value = filePath === null ? "" : serializeBinaryBodyPath(filePath);
  },
});

function beautifyBody() {
  if (rawLanguage.value === "json") {
    try {
      bodyText.value = beautifyJson(bodyText.value);
    } catch {
      // Leave it as-is — the editor's own JSON linter already flags what's
      // wrong; beautify has nothing useful to do with genuinely invalid JSON.
    }
  } else if (rawLanguage.value === "xml") {
    bodyText.value = formatXml(bodyText.value);
  }
}

// The variables drawer (#147): a read-only, hidden-by-default panel
// showing what this request's `{{variable}}` placeholders would actually
// resolve to for the currently-selected environment — collection
// variables and this project's session-chained variables included, via
// the same merge `sendRequest` uses — so a request with several
// placeholders doesn't require switching over to the environment editor
// just to check a value. Editing still only happens there; this is
// quick-reference only.
const variablesDrawerOpen = ref(false);
const resolvedVariables = ref<ResolvedVariables | null>(null);
const variablesLoading = ref(false);
const variablesError = ref<string | null>(null);

const sortedResolvedVariables = computed(() =>
  Object.entries(resolvedVariables.value?.variables ?? {}).sort(([a], [b]) => a.localeCompare(b)),
);

const secretVariableNames = computed(() => new Set(resolvedVariables.value?.secrets ?? []));

// Whether a masked variable's value is currently shown in plain text —
// purely local, ephemeral UI state (mirrors `KeyValueEditor`'s own
// `revealed` state for the environment editor), reset whenever the drawer
// reloads so a value doesn't stay revealed across requests/environments.
const revealedVariables = reactive<Record<string, boolean>>({});

function toggleVariableRevealed(name: string) {
  revealedVariables[name] = !revealedVariables[name];
}

async function loadResolvedVariables() {
  variablesLoading.value = true;
  variablesError.value = null;
  for (const name of Object.keys(revealedVariables)) delete revealedVariables[name];
  try {
    resolvedVariables.value = await getResolvedVariables(props.request.path, props.selectedEnvironment);
  } catch (e) {
    resolvedVariables.value = null;
    variablesError.value = String(e);
  } finally {
    variablesLoading.value = false;
  }
}

function toggleVariablesDrawer() {
  variablesDrawerOpen.value = !variablesDrawerOpen.value;
  if (variablesDrawerOpen.value) loadResolvedVariables();
}

// Keep the drawer's contents current with whichever request/environment is
// active while it's open, rather than only refreshing on the next manual
// toggle — `environmentsVersion` covers an edit made in EnvironmentPanel
// (e.g. adding a variable) landing here live, without the name of the
// active environment itself having changed.
watch([() => props.request.path, () => props.selectedEnvironment, () => props.environmentsVersion], () => {
  if (variablesDrawerOpen.value) loadResolvedVariables();
});

// "Copy as…" (#152): render this request, resolved the same way Send
// would resolve it, as a curl command or fetch() snippet, then copy it to
// the clipboard. The rendered text is always shown too — some webviews
// restrict clipboard writes, and this way a failed copy still leaves the
// user something to select by hand.
const copyAsMenuOpen = ref(false);
const copyAsPanelOpen = ref(false);
const copyAsFormat = ref<ExportFormat | null>(null);
const copyAsText = ref<string | null>(null);
const copyAsLoading = ref(false);
const copyAsError = ref<string | null>(null);
const copyAsJustCopied = ref(false);

const COPY_AS_OPTIONS: { format: ExportFormat; label: string }[] = [
  { format: "curl", label: "curl" },
  { format: "fetch", label: "fetch()" },
];

function toggleCopyAsMenu() {
  copyAsMenuOpen.value = !copyAsMenuOpen.value;
}

async function chooseCopyAsFormat(format: ExportFormat) {
  copyAsMenuOpen.value = false;
  copyAsPanelOpen.value = true;
  copyAsFormat.value = format;
  copyAsLoading.value = true;
  copyAsError.value = null;
  copyAsJustCopied.value = false;
  try {
    const text = await exportRequestAs(props.request.path, props.selectedEnvironment, format);
    copyAsText.value = text;
    try {
      await navigator.clipboard.writeText(text);
      copyAsJustCopied.value = true;
    } catch {
      // Clipboard write isn't available in every webview context — the
      // panel still shows the text for a manual copy.
    }
  } catch (e) {
    copyAsText.value = null;
    copyAsError.value = String(e);
  } finally {
    copyAsLoading.value = false;
  }
}

async function copyAsPanelCopyAgain() {
  if (!copyAsText.value) return;
  try {
    await navigator.clipboard.writeText(copyAsText.value);
    copyAsJustCopied.value = true;
  } catch (e) {
    copyAsError.value = String(e);
  }
}

function closeCopyAsPanel() {
  copyAsPanelOpen.value = false;
}

const curlPasteError = ref<string | null>(null);

// Pasting a curl/wget command into the URL field fills in method/URL/
// headers/body from it instead of treating the pasted text as a literal
// URL. Only intercepts the paste when the clipboard text actually looks
// like one (so an ordinary URL paste is completely unaffected); if parsing
// then fails anyway, falls back to inserting the raw pasted text so the
// paste isn't silently swallowed.
async function handleUrlPaste(event: ClipboardEvent) {
  const text = event.clipboardData?.getData("text") ?? "";
  if (!/^\s*(curl|wget)\b/i.test(text)) return;

  event.preventDefault();
  curlPasteError.value = null;
  try {
    const parsed = await parseCurlCommand(text);
    method.value = parsed.method;
    urlDisplay.value = parsed.url;
    headers.value = parsed.headers.map((h) => ({ ...h }));
    bodyText.value = parsed.body ?? "";
    await syncBodyTypeFromText();
  } catch (e) {
    urlDisplay.value = text;
    curlPasteError.value = String(e);
  }
}

const responseLanguage = computed<EditorLanguage>(() => {
  const contentType = response.value?.headers.find((h) => h.name.toLowerCase() === "content-type")?.value ?? "";
  return languageForContentType(contentType);
});

// Drives the Preview tab: HTML responses get rendered in a sandboxed iframe
// (like a browser would show them) rather than just syntax-highlighted text.
const isHtmlResponse = computed(() => {
  const contentType = response.value?.headers.find((h) => h.name.toLowerCase() === "content-type")?.value ?? "";
  const essence = contentType.split(";")[0]?.trim().toLowerCase() ?? "";
  return essence === "text/html";
});

// Pretty-print JSON response bodies before handing them to the editor;
// leave everything else (including malformed JSON) exactly as returned.
const responseBody = computed(() => {
  const body = response.value?.body ?? "";
  if (responseLanguage.value === "json") {
    try {
      return JSON.stringify(JSON.parse(body), null, 2);
    } catch {
      return body;
    }
  }
  return body;
});

/** Re-derives `bodyType`/`rawLanguage` from the current headers/`bodyText`, and refreshes whichever structured editor (multipart/GraphQL) that body type needs. */
async function syncBodyTypeFromText() {
  const detected = detectBodyType(headers.value, bodyText.value);
  bodyType.value = detected.type;
  rawLanguage.value = detected.rawLanguage;
  if (bodyType.value === "multipart") {
    await refreshMultipartFieldsFromText();
  } else if (bodyType.value === "graphql") {
    await refreshGraphqlFromText();
  } else {
    multipartFields.value = [];
    multipartParseError.value = null;
  }
}

// Populates the editable working-copy refs from a snapshot — shared by
// `load()` (a freshly-read draft) and `handleRevert()` (the same
// `original` snapshot the user is discarding their edits back to), so the
// two never drift out of sync with each other.
async function applyDraft(draft: RequestDraft) {
  method.value = draft.method;
  url.value = draft.url;
  query.value = draft.query.map((q) => ({ ...q }));
  headers.value = draft.headers.map((h) => ({ ...h }));
  bodyText.value = draft.body_text;
  auth.value = draft.auth ? { ...draft.auth } : null;
  syncContentType.value = draft.sync_content_type;
  assertText.value = draft.assert_text;
  scriptPre.value = draft.script_pre ?? "";
  scriptPost.value = draft.script_post ?? "";
  await syncBodyTypeFromText();
}

async function load() {
  loading.value = true;
  loadError.value = null;
  response.value = null;
  sendError.value = null;
  saveError.value = null;
  diffResult.value = null;
  diffError.value = null;
  try {
    const draft = await readRequest(props.request.path);
    // `url:` isn't supposed to carry its own query string, but nothing on
    // the engine side enforces that — if it does anyway, split it out here
    // rather than letting the URL field display the same params twice
    // (once embedded, once from `[params]`). `original` is normalized the
    // same way so dirty-tracking compares like with like.
    const { base, query: embeddedQuery } = splitUrlAndQuery(draft.url);
    const normalizedQuery = [...draft.query.map((q) => ({ ...q })), ...embeddedQuery];
    original.value = { ...draft, url: base, query: normalizedQuery };
    await applyDraft(original.value);
  } catch (e) {
    loadError.value = String(e);
    original.value = null;
  } finally {
    loading.value = false;
  }
}

/** Discards unsaved edits, restoring every field back to the last loaded/saved snapshot. */
async function handleRevert() {
  if (!original.value || !dirty.value) return;
  await applyDraft(original.value);
  saveError.value = null;
}

watch(
  () => props.request.path,
  () => {
    load();
  },
  { immediate: true },
);

async function handleSave(): Promise<boolean> {
  saving.value = true;
  saveError.value = null;
  try {
    // `has_example_response` comes along untouched from the last load: it
    // describes the example response the file already has, which saving
    // preserves rather than rewrites.
    const draft: RequestDraft = {
      ...(original.value as RequestDraft),
      method: method.value,
      url: url.value,
      query: query.value.map((q) => ({ ...q })),
      headers: headers.value.map((h) => ({ ...h })),
      body_text: bodyText.value,
      auth: auth.value ? { ...auth.value } : null,
      sync_content_type: syncContentType.value,
      assert_text: assertText.value,
      script_pre: scriptPre.value.trim() === "" ? null : scriptPre.value,
      script_post: scriptPost.value.trim() === "" ? null : scriptPost.value,
    };
    await saveRequest(props.request.path, draft);
    original.value = draft;
    emit("saved");
    return true;
  } catch (e) {
    saveError.value = String(e);
    return false;
  } finally {
    saving.value = false;
  }
}

async function handleSend() {
  // A dirty request would otherwise send stale on-disk content while
  // showing edited fields on screen — save first so Send never discards
  // (or ignores) an in-progress edit.
  if (dirty.value) {
    const saved = await handleSave();
    if (!saved) return;
  }

  sending.value = true;
  sendError.value = null;
  try {
    response.value = await sendRequest(props.request.path, props.selectedEnvironment);
    activeResponseTab.value = "preview";
  } catch (e) {
    response.value = null;
    sendError.value = String(e);
  } finally {
    sending.value = false;
  }
}

// "Save as Example" (#150): captures the response already sitting in
// `response` (from the send above) into this request's own `[response
// <status>]` section — no re-send involved.
const savingExample = ref(false);
const saveExampleError = ref<string | null>(null);
const saveExampleJustSaved = ref(false);
let saveExampleSavedTimeout: ReturnType<typeof setTimeout> | undefined;

async function handleSaveAsExample() {
  if (!response.value) return;

  savingExample.value = true;
  saveExampleError.value = null;
  saveExampleJustSaved.value = false;
  try {
    await saveResponseAsExample(props.request.path, response.value);
    if (original.value) original.value.has_example_response = true;
    // Refresh the "vs Saved Example" diff if it's the tab currently open,
    // since the example it compares against just changed.
    if (activeResponseTab.value === "diff" && diffMode.value === "example") loadDiff();

    saveExampleJustSaved.value = true;
    clearTimeout(saveExampleSavedTimeout);
    saveExampleSavedTimeout = setTimeout(() => {
      saveExampleJustSaved.value = false;
    }, 2000);
  } catch (e) {
    saveExampleError.value = String(e);
  } finally {
    savingExample.value = false;
  }
}

const responseSize = computed(() =>
  response.value ? new TextEncoder().encode(response.value.body).length : 0,
);

// Matches `.request-view__divider`'s CSS flex-basis, and the collapsed
// floor for either pane (`$pane-header-height` in _variables.scss).
const DIVIDER_HEIGHT = 7;
const PANE_HEADER_HEIGHT = 36;

// Shared across every open tab's `RequestPanel` instance (same localStorage
// keys as before this was extracted into a composable), matching how a
// single split-pane position feels regardless of which tab set it rather
// than each tab remembering its own.
const responsePane = useResizablePane({
  storageKey: "nova.responsePaneHeight",
  lastExpandedStorageKey: "nova.responsePaneLastExpandedHeight",
  defaultSize: 320,
  minSize: PANE_HEADER_HEIGHT,
  // The request pane's own floor is 96px normally, but drops to just its
  // header (36px) once it's explicitly collapsed — this is what lets the
  // response pane approach nearly the full height rather than always being
  // capped at "container - 96".
  getMax: (container) => {
    if (!container) return 320;
    const topMin = requestPaneCollapsed.value ? PANE_HEADER_HEIGHT : 96;
    return container.clientHeight - topMin - DIVIDER_HEIGHT;
  },
  axis: "vertical",
  direction: -1, // dragging up (decreasing clientY) grows the response pane
});
const isResponseCollapsed = responsePane.isCollapsed;

const REQUEST_PANE_COLLAPSED_KEY = "nova.requestPaneCollapsed";
const requestPaneCollapsed = ref(localStorage.getItem(REQUEST_PANE_COLLAPSED_KEY) === "true");
watch(requestPaneCollapsed, (value) => localStorage.setItem(REQUEST_PANE_COLLAPSED_KEY, String(value)));

// Collapsing one pane always force-expands the other if it's already
// collapsed — otherwise both could end up as two thin header bars with
// nothing actually visible.
function toggleRequestCollapsed() {
  const willCollapse = !requestPaneCollapsed.value;
  requestPaneCollapsed.value = willCollapse;
  if (willCollapse && responsePane.isCollapsed.value) responsePane.toggleCollapsed();
}

function toggleResponseCollapsed() {
  const wasCollapsed = responsePane.isCollapsed.value;
  responsePane.toggleCollapsed();
  if (wasCollapsed && requestPaneCollapsed.value) requestPaneCollapsed.value = false;
}

// Every open tab keeps its own live RequestPanel instance (see the
// `v-show` block in App.vue), so this listener is per-tab too — gate on
// `active` or Ctrl/Cmd+Enter in a background tab would send it as well.
function onGlobalKeydown(event: KeyboardEvent) {
  if (!props.active || event.key !== "Enter" || !(event.metaKey || event.ctrlKey)) return;
  event.preventDefault();
  handleSend();
}

// Closes the "Copy as…" menu on a click anywhere outside it, the same
// dismiss behavior a native `<select>` gets for free.
const copyAsMenuEl = ref<HTMLElement | null>(null);
function onGlobalMousedown(event: MouseEvent) {
  if (!copyAsMenuOpen.value) return;
  if (copyAsMenuEl.value && !copyAsMenuEl.value.contains(event.target as Node)) {
    copyAsMenuOpen.value = false;
  }
}

onMounted(() => {
  window.addEventListener("keydown", onGlobalKeydown);
  window.addEventListener("mousedown", onGlobalMousedown);
});

onBeforeUnmount(() => {
  window.removeEventListener("keydown", onGlobalKeydown);
  window.removeEventListener("mousedown", onGlobalMousedown);
  clearTimeout(saveExampleSavedTimeout);
});

defineExpose({ dirty, save: handleSave });
</script>

<template>
  <div class="request-view-shell">
  <div class="request-view" :ref="(el) => (responsePane.containerEl.value = el as HTMLElement | null)">
    <div
      class="request-view__pane request-view__pane--top"
      :class="{ 'request-view__pane--collapsed': requestPaneCollapsed }"
    >
    <div class="request-panel__header">
      <div class="request-panel__header-main">
        <button
          type="button"
          class="icon-button"
          :title="requestPaneCollapsed ? 'Expand request' : 'Collapse request'"
          @click="toggleRequestCollapsed"
        >
          <Icon name="chevron-down" :class="{ 'response-pane__collapse-icon--collapsed': requestPaneCollapsed }" />
        </button>
        <p class="request-panel__name">
          {{ request.name }}
          <span v-if="dirty" class="request-panel__dirty-dot" title="Unsaved changes"></span>
        </p>
      </div>
      <div class="request-panel__actions">
        <button
          type="button"
          class="button button--ghost"
          :class="{ 'button--ghost-active': variablesDrawerOpen }"
          title="Show what this request's {{variables}} resolve to"
          @click="toggleVariablesDrawer"
        >
          Variables
        </button>
        <button
          type="button"
          class="button button--ghost"
          title="Discard unsaved edits, back to the last saved version"
          :disabled="!dirty || saving"
          @click="handleRevert"
        >
          Revert
        </button>
        <button
          type="button"
          class="button button--secondary"
          :disabled="!dirty || saving"
          @click="handleSave"
        >
          {{ saving ? "Saving…" : "Save" }}
        </button>
      </div>
    </div>

    <div v-show="!requestPaneCollapsed" class="request-panel__body">
    <p v-if="loading" class="response-pane__hint">Loading request…</p>
    <p v-else-if="loadError" class="response-pane__error">{{ loadError }}</p>

    <template v-else-if="original">
      <p v-if="saveError" class="request-panel__save-error">Save failed: {{ saveError }}</p>

      <p v-if="original.has_example_response" class="request-panel__hint-text">
        This file also has an example response not shown here — saving preserves it as-is.
      </p>

      <div class="request-panel__method-url">
        <select v-model="method" class="request-panel__method-select">
          <option v-for="m in HTTP_METHODS" :key="m" :value="m">{{ m }}</option>
        </select>
        <input
          v-model="urlDisplay"
          type="text"
          class="request-panel__url-input"
          placeholder="{{base_url}}/path"
          @paste="handleUrlPaste"
          @keydown.enter="handleSend"
        />
        <button
          type="button"
          class="button request-panel__send"
          :disabled="sending || loading"
          @click="handleSend"
        >
          {{ sending ? "Sending…" : "Send" }}
        </button>
      </div>

      <p v-if="curlPasteError" class="request-panel__save-error">
        Couldn't parse the pasted curl command: {{ curlPasteError }}
      </p>

      <div class="request-panel__tabs-row">
        <div class="request-panel__tabs" role="tablist">
          <button
            type="button"
            role="tab"
            class="request-panel__tab"
            :class="{ 'request-panel__tab--active': activeTab === 'auth' }"
            :aria-selected="activeTab === 'auth'"
            @click="activeTab = 'auth'"
          >
            Auth<span v-if="auth" class="request-panel__tab-count">&bull;</span>
          </button>
          <button
            type="button"
            role="tab"
            class="request-panel__tab"
            :class="{ 'request-panel__tab--active': activeTab === 'headers' }"
            :aria-selected="activeTab === 'headers'"
            @click="activeTab = 'headers'"
          >
            Headers<span v-if="headers.length > 0" class="request-panel__tab-count">{{ headers.length }}</span>
          </button>
          <button
            type="button"
            role="tab"
            class="request-panel__tab"
            :class="{ 'request-panel__tab--active': activeTab === 'params' }"
            :aria-selected="activeTab === 'params'"
            @click="activeTab = 'params'"
          >
            Params<span v-if="query.length > 0" class="request-panel__tab-count">{{ query.length }}</span>
          </button>
          <button
            type="button"
            role="tab"
            class="request-panel__tab"
            :class="{ 'request-panel__tab--active': activeTab === 'body' }"
            :aria-selected="activeTab === 'body'"
            @click="activeTab = 'body'"
          >
            Body<span v-if="bodyText.trim().length > 0" class="request-panel__tab-count">&bull;</span>
          </button>
          <button
            type="button"
            role="tab"
            class="request-panel__tab"
            :class="{ 'request-panel__tab--active': activeTab === 'scripts' }"
            :aria-selected="activeTab === 'scripts'"
            @click="activeTab = 'scripts'"
          >
            Scripts<span
              v-if="scriptPre.trim().length > 0 || scriptPost.trim().length > 0"
              class="request-panel__tab-count"
              >&bull;</span
            >
          </button>
          <button
            type="button"
            role="tab"
            class="request-panel__tab"
            :class="{ 'request-panel__tab--active': activeTab === 'tests' }"
            :aria-selected="activeTab === 'tests'"
            @click="activeTab = 'tests'"
          >
            Tests<span v-if="assertText.trim().length > 0" class="request-panel__tab-count">&bull;</span>
          </button>
        </div>

        <div ref="copyAsMenuEl" class="copy-as">
          <button
            type="button"
            class="button button--ghost"
            title="Render this request as a curl command or code snippet"
            @click="toggleCopyAsMenu"
          >
            <Icon name="copy" />
            Copy as
            <Icon name="chevron-down" class="copy-as__chevron" />
          </button>
          <ul v-if="copyAsMenuOpen" class="copy-as__menu" role="menu">
            <li v-for="option in COPY_AS_OPTIONS" :key="option.format">
              <button
                type="button"
                role="menuitem"
                class="copy-as__menu-item"
                @click="chooseCopyAsFormat(option.format)"
              >
                {{ option.label }}
              </button>
            </li>
          </ul>
        </div>
      </div>

      <div v-if="copyAsPanelOpen" class="copy-as-panel">
        <div class="copy-as-panel__header">
          <p class="copy-as-panel__title">
            {{ copyAsFormat === "fetch" ? "fetch() snippet" : "curl command" }}
            <span v-if="copyAsJustCopied" class="copy-as-panel__copied">Copied to clipboard</span>
          </p>
          <div class="copy-as-panel__actions">
            <button
              type="button"
              class="button button--ghost"
              :disabled="!copyAsText"
              @click="copyAsPanelCopyAgain"
            >
              Copy
            </button>
            <button type="button" class="icon-button" title="Close" @click="closeCopyAsPanel">
              <Icon name="x" />
            </button>
          </div>
        </div>
        <p v-if="copyAsLoading" class="response-pane__hint">Rendering…</p>
        <p v-else-if="copyAsError" class="response-pane__error">{{ copyAsError }}</p>
        <pre v-else-if="copyAsText" class="copy-as-panel__text">{{ copyAsText }}</pre>
      </div>

      <div v-if="activeTab === 'auth'" class="request-panel__tab-panel">
        <AuthEditor v-model="auth" id-prefix="request-auth" :project-root="props.projectRoot" />
      </div>

      <div v-else-if="activeTab === 'headers'" class="request-panel__tab-panel">
        <KeyValueEditor v-model="headers" name-placeholder="Header" value-placeholder="Value" mode="headers" />
        <p class="request-panel__hint-text">
          Sent on every request, in addition to whatever's set above (only overridden if you set
          the same name yourself): <code>Host: &lt;from the URL&gt;</code>,
          <code>User-Agent: Nova/…</code>, <code>Accept: */*</code>,
          <code>Accept-Encoding: gzip</code>.
        </p>
      </div>

      <div v-else-if="activeTab === 'params'" class="request-panel__tab-panel">
        <KeyValueEditor v-model="query" name-placeholder="param" value-placeholder="value" />
      </div>

      <div v-else-if="activeTab === 'body'" class="request-panel__tab-panel">
        <div class="request-panel__body-type">
          <div class="request-panel__body-type-radios" role="radiogroup" aria-label="Body type">
            <label v-for="option in BODY_TYPE_OPTIONS" :key="option" class="request-panel__radio">
              <input
                type="radio"
                name="body-type"
                :value="option"
                :checked="bodyType === option"
                @change="handleBodyTypeChange(option)"
              />
              {{ BODY_TYPE_LABELS[option] }}
            </label>
          </div>
          <select
            v-if="bodyType === 'raw'"
            class="request-panel__body-type-select"
            :value="rawLanguage"
            @change="handleRawLanguageChange(($event.target as HTMLSelectElement).value as RawLanguage)"
          >
            <option v-for="lang in RAW_LANGUAGE_OPTIONS" :key="lang" :value="lang">
              {{ RAW_LANGUAGE_LABELS[lang] }}
            </option>
          </select>
          <label class="request-panel__body-setting">
            <input v-model="syncContentType" type="checkbox" />
            Keep the Content-Type header in sync
          </label>
        </div>
        <p v-if="!syncContentType" class="request-panel__hint-text">
          Selecting a body type won't change the Content-Type header — set it yourself on the
          Headers tab.
        </p>
        <p v-if="bodyType === 'none'" class="request-panel__hint-text">This request has no body.</p>
        <KeyValueEditor
          v-else-if="bodyType === 'form'"
          v-model="formFields"
          name-placeholder="key"
          value-placeholder="value"
        />
        <template v-else-if="bodyType === 'multipart'">
          <p v-if="multipartParseError" class="request-panel__save-error">
            Couldn't parse this request's existing multipart body: {{ multipartParseError }}. Editing
            here will replace it.
          </p>
          <MultipartEditor v-model="multipartFields" :project-root="projectRoot" />
        </template>
        <BinaryEditor
          v-else-if="bodyType === 'binary'"
          v-model="binaryFilePath"
          :project-root="projectRoot"
        />
        <div v-else-if="bodyType === 'graphql'" class="request-panel__graphql">
          <div class="request-panel__graphql-pane">
            <span class="request-panel__graphql-label">Query</span>
            <CodeEditor v-model="graphqlQuery" language="text" />
          </div>
          <div class="request-panel__graphql-pane">
            <span class="request-panel__graphql-label">Variables</span>
            <CodeEditor v-model="graphqlVariablesText" language="json" />
            <p v-if="graphqlVariablesError" class="request-panel__save-error">{{ graphqlVariablesError }}</p>
          </div>
        </div>
        <div v-else-if="bodyType === 'raw'" class="request-panel__body-editor">
          <CodeEditor v-model="bodyText" :language="editorLanguage" />
          <button
            v-if="rawLanguage === 'json' || rawLanguage === 'xml'"
            type="button"
            class="icon-button icon-button--outline request-panel__beautify"
            title="Beautify"
            @click="beautifyBody"
          >
            <Icon name="wand" />
          </button>
        </div>
      </div>

      <div v-else-if="activeTab === 'scripts'" class="request-panel__tab-panel">
        <p class="request-panel__hint-text">
          Named scripts run around this request's execution — a
          <code>pre:</code> script can add or override headers/params or
          replace the body, and a <code>post:</code> script can extract
          <code>&#123;&#123;variable&#125;&#125;</code> values for later
          requests in the same run. A bare name resolves under
          <code>nova/scripts/</code>;
          an explicit path is relative to the project root. A collection or
          folder can also carry its own scripts that wrap around this
          request's — see the project's <code>_collection.yaml</code>.
        </p>
        <label class="request-panel__script-field">
          <span class="request-panel__script-label">pre:</span>
          <input
            v-model="scriptPre"
            type="text"
            class="request-panel__script-input"
            placeholder="sign-request.py"
          />
        </label>
        <label class="request-panel__script-field">
          <span class="request-panel__script-label">post:</span>
          <input
            v-model="scriptPost"
            type="text"
            class="request-panel__script-input"
            placeholder="log-response.js"
          />
        </label>
      </div>

      <div v-else-if="activeTab === 'tests'" class="request-panel__tab-panel">
        <p class="request-panel__hint-text">
          One assertion or extraction per line: <code>status == 200</code>,
          <code>response.user.id exists</code>,
          <code>access_token = response.access_token</code>. Runs with
          <code>nova test</code> or the Run Tests button in the top bar.
        </p>
        <CodeEditor v-model="assertText" language="text" />
      </div>
    </template>
    </div>
    </div>

    <div class="request-view__divider" title="Drag to resize" @mousedown="responsePane.startDrag"></div>

    <div
      class="request-view__pane request-view__pane--bottom"
      :class="{ 'request-view__pane--collapsed': isResponseCollapsed }"
      :style="{ flexBasis: `${responsePane.size.value}px` }"
    >
    <div class="response-pane__header">
      <span class="response-pane__header-label">Response</span>
      <button
        type="button"
        class="icon-button"
        :title="isResponseCollapsed ? 'Expand response' : 'Collapse response'"
        @click="toggleResponseCollapsed"
      >
        <Icon name="chevron-down" :class="{ 'response-pane__collapse-icon--collapsed': isResponseCollapsed }" />
      </button>
    </div>
    <div v-show="!isResponseCollapsed" class="response-pane">
      <p v-if="sending" class="response-pane__hint">Sending request…</p>

      <p v-else-if="sendError" class="response-pane__error">{{ sendError }}</p>

      <template v-else-if="response">
        <div class="response-summary">
          <span class="response-status" :class="statusClass(response.status)">
            {{ response.status }}
          </span>
          <span class="response-summary__meta">Time <strong>{{ response.elapsed_ms }} ms</strong></span>
          <span class="response-summary__meta">Size <strong>{{ formatBytes(responseSize) }}</strong></span>
          <button
            type="button"
            class="button button--ghost response-summary__save-example"
            title="Capture this response into the request's [response] section"
            :disabled="savingExample"
            @click="handleSaveAsExample"
          >
            {{ savingExample ? "Saving…" : "Save as Example" }}
          </button>
          <span v-if="saveExampleJustSaved" class="response-summary__save-example-saved">Saved</span>
        </div>
        <p v-if="saveExampleError" class="response-pane__error">{{ saveExampleError }}</p>

        <div class="request-panel__tabs" role="tablist">
          <button
            type="button"
            role="tab"
            class="request-panel__tab"
            :class="{ 'request-panel__tab--active': activeResponseTab === 'headers' }"
            :aria-selected="activeResponseTab === 'headers'"
            @click="activeResponseTab = 'headers'"
          >
            Headers<span v-if="response.headers.length > 0" class="request-panel__tab-count">{{
              response.headers.length
            }}</span>
          </button>
          <button
            type="button"
            role="tab"
            class="request-panel__tab"
            :class="{ 'request-panel__tab--active': activeResponseTab === 'raw' }"
            :aria-selected="activeResponseTab === 'raw'"
            @click="activeResponseTab = 'raw'"
          >
            Raw Response
          </button>
          <button
            type="button"
            role="tab"
            class="request-panel__tab"
            :class="{ 'request-panel__tab--active': activeResponseTab === 'preview' }"
            :aria-selected="activeResponseTab === 'preview'"
            @click="activeResponseTab = 'preview'"
          >
            Preview
          </button>
          <button
            type="button"
            role="tab"
            class="request-panel__tab"
            :class="{ 'request-panel__tab--active': activeResponseTab === 'diff' }"
            :aria-selected="activeResponseTab === 'diff'"
            @click="activeResponseTab = 'diff'"
          >
            Diff
          </button>
        </div>

        <div v-if="activeResponseTab === 'headers'" class="request-panel__tab-panel">
          <ul v-if="response.headers.length > 0" class="response-headers">
            <li v-for="header in response.headers" :key="header.name" class="response-headers__item">
              <span class="response-headers__name">{{ header.name }}</span>
              <span class="response-headers__value">{{ header.value }}</span>
            </li>
          </ul>
          <p v-else class="response-pane__hint">No headers.</p>
        </div>

        <div v-else-if="activeResponseTab === 'raw'" class="request-panel__tab-panel">
          <CodeEditor v-if="response.body" :model-value="response.body" language="text" readonly />
          <p v-else class="response-pane__hint">Empty body.</p>
        </div>

        <div v-else-if="activeResponseTab === 'preview'" class="request-panel__tab-panel">
          <iframe
            v-if="response.body && isHtmlResponse"
            class="response-preview-frame"
            sandbox=""
            :srcdoc="response.body"
          ></iframe>
          <CodeEditor
            v-else-if="response.body"
            :model-value="responseBody"
            :language="responseLanguage"
            readonly
          />
          <p v-else class="response-pane__hint">Empty body.</p>
        </div>

        <div v-else class="request-panel__tab-panel">
          <div class="response-diff__toggle">
            <button
              type="button"
              :class="diffMode === 'previous' ? 'button' : 'button--secondary'"
              @click="diffMode = 'previous'"
            >
              vs Previous Run
            </button>
            <button
              v-if="hasExampleResponse"
              type="button"
              :class="diffMode === 'example' ? 'button' : 'button--secondary'"
              @click="diffMode = 'example'"
            >
              vs Saved Example
            </button>
          </div>

          <p v-if="diffLoading" class="response-pane__hint">Loading diff…</p>
          <p v-else-if="diffError" class="response-pane__error">{{ diffError }}</p>
          <p v-else-if="!diffResult && diffMode === 'previous'" class="response-pane__hint">
            No previous send of this request yet this session to compare against — send it again to
            start comparing.
          </p>
          <p v-else-if="!diffResult" class="response-pane__hint">
            This request has no saved <code>[response]</code> example to compare against.
          </p>
          <ResponseDiffView v-else :diff="diffResult" />
        </div>
      </template>

      <p v-else class="response-pane__hint">Click Send to execute this request.</p>
    </div>
    </div>
  </div>

  <aside class="variables-drawer" :class="{ 'variables-drawer--open': variablesDrawerOpen }">
    <div class="variables-drawer__inner">
      <p class="variables-drawer__title">Variables</p>
      <p v-if="variablesLoading" class="response-pane__hint">Resolving variables…</p>
      <p v-else-if="variablesError" class="response-pane__error">{{ variablesError }}</p>
      <p v-else-if="sortedResolvedVariables.length === 0" class="response-pane__hint">
        No variables resolve for this request's active environment.
      </p>
      <ul v-else class="variables-drawer__list">
        <li v-for="[name, value] in sortedResolvedVariables" :key="name" class="variables-drawer__item">
          <span class="variables-drawer__name">{{ name }}</span>
          <span
            class="variables-drawer__value"
            :class="{ 'variables-drawer__value--masked': secretVariableNames.has(name) && !revealedVariables[name] }"
          >{{ secretVariableNames.has(name) && !revealedVariables[name] ? "••••••••" : value }}</span>
          <button
            v-if="secretVariableNames.has(name)"
            type="button"
            class="variables-drawer__reveal"
            :title="revealedVariables[name] ? 'Hide value' : 'Reveal value'"
            @click="toggleVariableRevealed(name)"
          >
            <Icon :name="revealedVariables[name] ? 'eye-off' : 'eye'" />
          </button>
        </li>
      </ul>
      <p class="request-panel__hint-text">
        Read-only — edit values in the environment editor instead.
      </p>
    </div>
  </aside>
  </div>
</template>
