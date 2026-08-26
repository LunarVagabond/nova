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
  const contentType = headers.value.find((h) => h.name.toLowerCase() === "content-type")?.value ?? "";
  const essence = contentType.split(";")[0]?.trim().toLowerCase();
  if (essence === "application/json") return "json";
  if (essence === "application/xml" || essence === "text/xml") return "xml";
  return "text";
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

      <div class="request-panel__field-group">
        <span class="request-panel__field-label">Query Params</span>
        <KeyValueEditor v-model="query" name-placeholder="param" value-placeholder="value" />
      </div>

      <div class="request-panel__field-group">
        <span class="request-panel__field-label">Headers</span>
        <KeyValueEditor v-model="headers" name-placeholder="Header" value-placeholder="Value" mode="headers" />
      </div>

      <div class="request-panel__field-group">
        <span class="request-panel__field-label">Body</span>
        <CodeEditor v-model="bodyText" :language="editorLanguage" />
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
        <pre v-if="response.body" class="response-body">{{ response.body }}</pre>
        <p v-else class="response-pane__hint">Empty body.</p>
      </template>

      <p v-else class="response-pane__hint">Click Send to execute this request.</p>
    </div>
  </div>
</template>
