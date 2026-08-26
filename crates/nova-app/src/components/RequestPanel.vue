<script setup lang="ts">
import { computed, ref, watch } from "vue";

import { readRequest, saveRequest, sendRequest } from "../api/nova";
import type { QueryParam, RequestDraft, RequestFile, RequestHeader, RequestResponse } from "../types/nova";
import CodeEditor, { type EditorLanguage } from "./CodeEditor.vue";
import KeyValueEditor from "./KeyValueEditor.vue";

const HTTP_METHODS = ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];

const props = defineProps<{
  request: RequestFile;
  selectedEnvironment: string | null;
}>();

const emit = defineEmits<{
  (e: "dirtyChange", dirty: boolean): void;
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

type FieldTab = "params" | "headers" | "body";
const activeTab = ref<FieldTab>("params");

type BodyType = "none" | "json" | "xml" | "form" | "multipart" | "text";

const BODY_TYPE_OPTIONS: BodyType[] = ["none", "json", "xml", "form", "multipart", "text"];

const BODY_TYPE_LABELS: Record<BodyType, string> = {
  none: "No Body",
  json: "JSON",
  xml: "XML",
  form: "Form URL Encoded",
  multipart: "Multipart Form Data",
  text: "Plain Text",
};

const BODY_TYPE_CONTENT_TYPES: Record<Exclude<BodyType, "none">, string> = {
  json: "application/json",
  xml: "application/xml",
  form: "application/x-www-form-urlencoded",
  multipart: "multipart/form-data",
  text: "text/plain",
};

// Driven by the Content-Type header (and whether there's any body text at
// all) — set explicitly by `handleBodyTypeChange` below, and re-derived
// whenever a request is (re)loaded so it reflects what the file actually
// has, not just the last selection made in this session.
const bodyType = ref<BodyType>("none");

function detectBodyType(currentHeaders: RequestHeader[], text: string): BodyType {
  if (text.trim() === "") return "none";
  const contentType = currentHeaders.find((h) => h.name.toLowerCase() === "content-type")?.value ?? "";
  const essence = contentType.split(";")[0]?.trim().toLowerCase() ?? "";
  if (essence === "application/json" || essence.endsWith("+json")) return "json";
  if (essence === "application/xml" || essence === "text/xml" || essence.endsWith("+xml")) return "xml";
  if (essence === "application/x-www-form-urlencoded") return "form";
  if (essence === "multipart/form-data") return "multipart";
  return "text";
}

function randomBoundary(): string {
  return `----NovaBoundary${Math.random().toString(16).slice(2)}`;
}

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

// Selecting a body type is the source of truth for the Content-Type
// header: "No Body" clears both the text and the header entirely, and
// every other option sets the header to its canonical value (a fresh
// boundary for multipart) so the two never disagree.
function handleBodyTypeChange(next: BodyType) {
  if (next === bodyType.value) return;
  bodyType.value = next;

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
    bodyText.value !== original.value.body_text
  );
});

watch(dirty, (value) => emit("dirtyChange", value));

const editorLanguage = computed<EditorLanguage>(() => {
  if (bodyType.value === "json") return "json";
  if (bodyType.value === "xml") return "xml";
  return "text";
});

// Shared with the response pane below: a `+json`/`+xml` structured syntax
// suffix (e.g. `application/vnd.api+json`) is treated the same as the bare
// media type.
function languageForContentType(contentType: string): EditorLanguage {
  const essence = contentType.split(";")[0]?.trim().toLowerCase() ?? "";
  if (essence === "application/json" || essence.endsWith("+json")) return "json";
  if (essence === "application/xml" || essence === "text/xml" || essence.endsWith("+xml")) return "xml";
  return "text";
}

const responseLanguage = computed<EditorLanguage>(() => {
  const contentType = response.value?.headers.find((h) => h.name.toLowerCase() === "content-type")?.value ?? "";
  return languageForContentType(contentType);
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
    original.value = draft;
    method.value = draft.method;
    url.value = draft.url;
    query.value = draft.query.map((q) => ({ ...q }));
    headers.value = draft.headers.map((h) => ({ ...h }));
    bodyText.value = draft.body_text;
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
    await saveRequest(props.request.path, {
      method: method.value,
      url: url.value,
      query: query.value,
      headers: headers.value,
      body: bodyText.value,
    });
    original.value = {
      ...(original.value as RequestDraft),
      method: method.value,
      url: url.value,
      query: query.value.map((q) => ({ ...q })),
      headers: headers.value.map((h) => ({ ...h })),
      body_text: bodyText.value,
    };
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
  } catch (e) {
    response.value = null;
    sendError.value = String(e);
  } finally {
    sending.value = false;
  }
}

function statusClass(status: number): string {
  if (status >= 200 && status < 300) return "response-status--ok";
  if (status >= 400) return "response-status--error";
  return "response-status--other";
}

defineExpose({ dirty, save: handleSave });
</script>

<template>
  <div>
    <div class="request-panel__header">
      <div>
        <p class="request-panel__name">
          {{ request.name }}
          <span v-if="dirty" class="request-panel__dirty-dot" title="Unsaved changes"></span>
        </p>
        <p class="request-panel__path">{{ request.path }}</p>
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
        <button class="button" :disabled="sending || loading" @click="handleSend">
          {{ sending ? "Sending…" : "Send" }}
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
          v-model="url"
          type="text"
          class="request-panel__url-input"
          placeholder="{{base_url}}/path"
        />
      </div>

      <div class="request-panel__tabs" role="tablist">
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
          :class="{ 'request-panel__tab--active': activeTab === 'body' }"
          :aria-selected="activeTab === 'body'"
          @click="activeTab = 'body'"
        >
          Body<span v-if="bodyText.trim().length > 0" class="request-panel__tab-count">&bull;</span>
        </button>
      </div>

      <div v-if="activeTab === 'params'" class="request-panel__tab-panel">
        <KeyValueEditor v-model="query" name-placeholder="param" value-placeholder="value" />
      </div>

      <div v-else-if="activeTab === 'headers'" class="request-panel__tab-panel">
        <KeyValueEditor v-model="headers" name-placeholder="Header" value-placeholder="Value" mode="headers" />
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
        </div>
        <p v-if="bodyType === 'none'" class="request-panel__hint-text">This request has no body.</p>
        <CodeEditor v-else v-model="bodyText" :language="editorLanguage" />
      </div>
    </template>

    <div class="response-pane">
      <p v-if="sending" class="response-pane__hint">Sending request…</p>

      <p v-else-if="sendError" class="response-pane__error">{{ sendError }}</p>

      <template v-else-if="response">
        <div class="response-summary">
          <span class="response-status" :class="statusClass(response.status)">
            {{ response.status }}
          </span>
          <span class="response-summary__elapsed">{{ response.elapsed_ms }}ms</span>
        </div>

        <h3 class="response-pane__section-title">Headers</h3>
        <ul v-if="response.headers.length > 0" class="response-headers">
          <li v-for="header in response.headers" :key="header.name" class="response-headers__item">
            <span class="response-headers__name">{{ header.name }}</span>
            <span class="response-headers__value">{{ header.value }}</span>
          </li>
        </ul>
        <p v-else class="response-pane__hint">No headers.</p>

        <h3 class="response-pane__section-title">Body</h3>
        <CodeEditor
          v-if="response.body"
          :model-value="responseBody"
          :language="responseLanguage"
          readonly
        />
        <p v-else class="response-pane__hint">Empty body.</p>
      </template>

      <p v-else class="response-pane__hint">Click Send to execute this request.</p>
    </div>
  </div>
</template>
