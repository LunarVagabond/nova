<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";

import { parseCurlCommand, readRequest, saveRequest, sendRequest } from "../api/nova";
import type {
  AuthScheme,
  QueryParam,
  RequestDraft,
  RequestFile,
  RequestHeader,
  RequestResponse,
} from "../types/nova";
import {
  BODY_TYPE_CONTENT_TYPES,
  BODY_TYPE_LABELS,
  BODY_TYPE_OPTIONS,
  detectBodyType,
  languageForContentType,
  randomBoundary,
  type BodyType,
} from "../lib/bodyType";
import { formatBytes, statusClass } from "../lib/format";
import { parseQueryString, serializeQuery, splitUrlAndQuery } from "../lib/queryString";
import { beautifyJson } from "../lib/jsonFormat";
import { formatXml } from "../lib/xmlFormat";
import AuthEditor from "./AuthEditor.vue";
import CodeEditor, { type EditorLanguage } from "./CodeEditor.vue";
import Icon from "./Icon.vue";
import KeyValueEditor from "./KeyValueEditor.vue";

const HTTP_METHODS = ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];

const props = defineProps<{
  request: RequestFile;
  selectedEnvironment: string | null;
  active: boolean;
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

type FieldTab = "auth" | "headers" | "params" | "body";
const activeTab = ref<FieldTab>("auth");

type ResponseTab = "headers" | "raw" | "preview";
const activeResponseTab = ref<ResponseTab>("preview");

// Driven by the Content-Type header (and whether there's any body text at
// all) — set explicitly by `handleBodyTypeChange` below, and re-derived
// whenever a request is (re)loaded so it reflects what the file actually
// has, not just the last selection made in this session.
const bodyType = ref<BodyType>("none");

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

  if (!syncContentType.value) return;

  if (next === "none") {
    bodyText.value = "";
    upsertContentTypeHeader(null);
    return;
  }

  upsertContentTypeHeader(
    next === "multipart" ? `multipart/form-data; boundary=${randomBoundary()}` : BODY_TYPE_CONTENT_TYPES[next],
  );
}

const saving = ref(false);
const saveError = ref<string | null>(null);

const sending = ref(false);
const sendError = ref<string | null>(null);
const response = ref<RequestResponse | null>(null);

const dirty = computed(() => {
  if (!original.value) return false;
  return (
    method.value !== original.value.method ||
    url.value !== original.value.url ||
    JSON.stringify(query.value) !== JSON.stringify(original.value.query) ||
    JSON.stringify(headers.value) !== JSON.stringify(original.value.headers) ||
    bodyText.value !== original.value.body_text ||
    JSON.stringify(auth.value) !== JSON.stringify(original.value.auth) ||
    syncContentType.value !== original.value.sync_content_type
  );
});

watch(dirty, (value) => emit("dirtyChange", value));

const editorLanguage = computed<EditorLanguage>(() => {
  if (bodyType.value === "json") return "json";
  if (bodyType.value === "xml") return "xml";
  return "text";
});

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

function beautifyBody() {
  if (bodyType.value === "json") {
    try {
      bodyText.value = beautifyJson(bodyText.value);
    } catch {
      // Leave it as-is — the editor's own JSON linter already flags what's
      // wrong; beautify has nothing useful to do with genuinely invalid JSON.
    }
  } else if (bodyType.value === "xml") {
    bodyText.value = formatXml(bodyText.value);
  }
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
    bodyType.value = detectBodyType(headers.value, bodyText.value);
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

async function load() {
  loading.value = true;
  loadError.value = null;
  response.value = null;
  sendError.value = null;
  saveError.value = null;
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
    method.value = draft.method;
    url.value = base;
    query.value = normalizedQuery.map((q) => ({ ...q }));
    headers.value = draft.headers.map((h) => ({ ...h }));
    bodyText.value = draft.body_text;
    auth.value = draft.auth ? { ...draft.auth } : null;
    syncContentType.value = draft.sync_content_type;
    bodyType.value = detectBodyType(headers.value, bodyText.value);
  } catch (e) {
    loadError.value = String(e);
    original.value = null;
  } finally {
    loading.value = false;
  }
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
    // `has_*` come along untouched from the last load: they describe the
    // assertions/example response the file already has, which saving
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

const responseSize = computed(() =>
  response.value ? new TextEncoder().encode(response.value.body).length : 0,
);

// Height (px) of the response pane below the draggable divider — shared
// across every open tab's `RequestPanel` instance (they read/write the same
// key), matching how a single split-pane position feels in Postman/VS Code
// rather than each tab remembering its own.
const RESPONSE_HEIGHT_KEY = "nova.responsePaneHeight";
const responseHeight = ref(Number(localStorage.getItem(RESPONSE_HEIGHT_KEY)) || 320);
let dragging = false;

function startDrag(event: MouseEvent) {
  dragging = true;
  const startY = event.clientY;
  const startHeight = responseHeight.value;
  document.body.style.cursor = "row-resize";

  function onMove(moveEvent: MouseEvent) {
    if (!dragging) return;
    const delta = startY - moveEvent.clientY;
    responseHeight.value = Math.min(Math.max(startHeight + delta, 120), window.innerHeight - 200);
  }
  function onUp() {
    dragging = false;
    document.body.style.cursor = "";
    localStorage.setItem(RESPONSE_HEIGHT_KEY, String(responseHeight.value));
    window.removeEventListener("mousemove", onMove);
    window.removeEventListener("mouseup", onUp);
  }
  window.addEventListener("mousemove", onMove);
  window.addEventListener("mouseup", onUp);
  event.preventDefault();
}

// Every open tab keeps its own live RequestPanel instance (see the
// `v-show` block in App.vue), so this listener is per-tab too — gate on
// `active` or Ctrl/Cmd+Enter in a background tab would send it as well.
function onGlobalKeydown(event: KeyboardEvent) {
  if (!props.active || event.key !== "Enter" || !(event.metaKey || event.ctrlKey)) return;
  event.preventDefault();
  handleSend();
}

onMounted(() => {
  window.addEventListener("keydown", onGlobalKeydown);
});

onBeforeUnmount(() => {
  dragging = false;
  window.removeEventListener("keydown", onGlobalKeydown);
});

defineExpose({ dirty, save: handleSave });
</script>

<template>
  <div class="request-view">
    <div class="request-view__pane request-view__pane--top">
    <div class="request-panel__header">
      <div>
        <p class="request-panel__name">
          {{ request.name }}
          <span v-if="dirty" class="request-panel__dirty-dot" title="Unsaved changes"></span>
        </p>
      </div>
      <div class="request-panel__actions">
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

    <p v-if="loading" class="response-pane__hint">Loading request…</p>
    <p v-else-if="loadError" class="response-pane__error">{{ loadError }}</p>

    <template v-else-if="original">
      <p v-if="saveError" class="request-panel__save-error">Save failed: {{ saveError }}</p>

      <p
        v-if="original.has_assertions || original.has_extractions || original.has_example_response"
        class="request-panel__hint-text"
      >
        This file also has
        <template v-if="original.has_assertions || original.has_extractions">assertions/extractions</template>
        <template v-if="(original.has_assertions || original.has_extractions) && original.has_example_response">
          and</template>
        <template v-if="original.has_example_response"> an example response</template>
        not shown here — saving preserves them as-is.
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
      </div>

      <div v-if="activeTab === 'auth'" class="request-panel__tab-panel">
        <AuthEditor v-model="auth" id-prefix="request-auth" />
      </div>

      <div v-else-if="activeTab === 'headers'" class="request-panel__tab-panel">
        <KeyValueEditor v-model="headers" name-placeholder="Header" value-placeholder="Value" mode="headers" />
        <p class="request-panel__hint-text">
          Sent automatically if not set above: <code>User-Agent: Nova/…</code>,
          <code>Accept: */*</code>.
        </p>
      </div>

      <div v-else-if="activeTab === 'params'" class="request-panel__tab-panel">
        <KeyValueEditor v-model="query" name-placeholder="param" value-placeholder="value" />
      </div>

      <div v-else class="request-panel__tab-panel">
        <div class="request-panel__body-type">
          <span class="request-panel__body-type-label">Content-Type</span>
          <select
            class="request-panel__body-type-select"
            :value="bodyType"
            @change="handleBodyTypeChange(($event.target as HTMLSelectElement).value as BodyType)"
          >
            <option v-for="option in BODY_TYPE_OPTIONS" :key="option" :value="option">
              {{ BODY_TYPE_LABELS[option] }}
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
          <p class="request-panel__hint-text">
            Edited as raw multipart text for now — a structured field editor with real file
            attachments is tracked separately (see the project's issue tracker); the format
            itself (name/filename/Content-Type per part) is already fully supported end to end.
          </p>
          <CodeEditor v-model="bodyText" language="text" />
        </template>
        <div v-else class="request-panel__body-editor">
          <CodeEditor v-model="bodyText" :language="editorLanguage" />
          <button
            v-if="bodyType === 'json' || bodyType === 'xml'"
            type="button"
            class="icon-button icon-button--outline request-panel__beautify"
            title="Beautify"
            @click="beautifyBody"
          >
            <Icon name="wand" />
          </button>
        </div>
      </div>
    </template>
    </div>

    <div class="request-view__divider" title="Drag to resize" @mousedown="startDrag"></div>

    <div class="request-view__pane request-view__pane--bottom" :style="{ flexBasis: `${responseHeight}px` }">
    <div class="response-pane">
      <p v-if="sending" class="response-pane__hint">Sending request…</p>

      <p v-else-if="sendError" class="response-pane__error">{{ sendError }}</p>

      <template v-else-if="response">
        <div class="response-summary">
          <span class="response-status" :class="statusClass(response.status)">
            {{ response.status }}
          </span>
          <span class="response-summary__meta">Time <strong>{{ response.elapsed_ms }} ms</strong></span>
          <span class="response-summary__meta">Size <strong>{{ formatBytes(responseSize) }}</strong></span>
        </div>

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

        <div v-else class="request-panel__tab-panel">
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
      </template>

      <p v-else class="response-pane__hint">Click Send to execute this request.</p>
    </div>
    </div>
  </div>
</template>
