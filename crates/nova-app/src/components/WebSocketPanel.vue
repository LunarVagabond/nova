<script setup lang="ts">
import { computed, ref, watch } from "vue";

import { connectWebSocket, readWebSocketRequest, saveWebSocketRequest } from "../api/nova";
import type { RequestFile, RequestHeader, WebSocketExchange } from "../types/nova";
import Icon from "./Icon.vue";
import KeyValueEditor from "./KeyValueEditor.vue";

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

// Mirrors `RequestPanel`'s `original`/editable-working-copy split: `original`
// is the last loaded/saved snapshot, the refs below are what the form edits,
// and `dirty` is a plain comparison between the two.
const original = ref<{ url: string; headers: RequestHeader[]; messages: string[] } | null>(null);

const url = ref("");
const headers = ref<RequestHeader[]>([]);
const messages = ref<string[]>([]);

const dirty = computed(() => {
  if (!original.value) return false;
  return (
    url.value !== original.value.url ||
    JSON.stringify(headers.value) !== JSON.stringify(original.value.headers) ||
    JSON.stringify(messages.value) !== JSON.stringify(original.value.messages)
  );
});

watch(dirty, (value) => emit("dirtyChange", value));

function applyDraft(draft: { url: string; headers: RequestHeader[]; messages: string[] }) {
  url.value = draft.url;
  headers.value = draft.headers.map((h) => ({ ...h }));
  messages.value = [...draft.messages];
}

// Declared before `load()` (below) rather than near their own
// save/connect handlers, since `load()` resets them and is invoked
// immediately by the `watch(..., { immediate: true })` a few lines down —
// referencing a `const` before its declaration in the same module scope
// would otherwise throw a temporal-dead-zone error the very first time a
// tab opens.
const saving = ref(false);
const saveError = ref<string | null>(null);
const connecting = ref(false);
const connectError = ref<string | null>(null);
const exchange = ref<WebSocketExchange | null>(null);

async function load() {
  loading.value = true;
  loadError.value = null;
  exchange.value = null;
  connectError.value = null;
  saveError.value = null;
  try {
    const draft = await readWebSocketRequest(props.request.path);
    original.value = draft;
    applyDraft(draft);
  } catch (e) {
    loadError.value = String(e);
    original.value = null;
  } finally {
    loading.value = false;
  }
}

watch(
  () => props.request.path,
  () => load(),
  { immediate: true },
);

async function handleRevert() {
  if (!original.value || !dirty.value) return;
  applyDraft(original.value);
  saveError.value = null;
}

async function handleSave(): Promise<boolean> {
  saving.value = true;
  saveError.value = null;
  try {
    const draft = {
      url: url.value,
      headers: headers.value.map((h) => ({ ...h })),
      messages: [...messages.value],
    };
    await saveWebSocketRequest(props.request.path, draft);
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

async function handleConnect() {
  // Same "save before send" rule as `RequestPanel`'s Send button: connecting
  // against stale on-disk content while the form shows edited fields would
  // be confusing, so save first if there are unsaved edits.
  if (dirty.value) {
    const saved = await handleSave();
    if (!saved) return;
  }

  connecting.value = true;
  connectError.value = null;
  exchange.value = null;
  try {
    exchange.value = await connectWebSocket(props.request.path, props.selectedEnvironment);
  } catch (e) {
    exchange.value = null;
    connectError.value = String(e);
  } finally {
    connecting.value = false;
  }
}

// Message list editing — a flat ordered list of plain-text lines, not a
// name/value table, so `KeyValueEditor` doesn't fit; this is intentionally
// a minimal add/remove/reorder editor rather than a general-purpose one.
function addMessage() {
  messages.value = [...messages.value, ""];
}

function updateMessage(index: number, value: string) {
  messages.value = messages.value.map((m, i) => (i === index ? value : m));
}

function removeMessage(index: number) {
  messages.value = messages.value.filter((_, i) => i !== index);
}

function moveMessage(index: number, direction: -1 | 1) {
  const target = index + direction;
  if (target < 0 || target >= messages.value.length) return;
  const next = [...messages.value];
  [next[index], next[target]] = [next[target], next[index]];
  messages.value = next;
}

defineExpose({ dirty, save: handleSave });
</script>

<template>
  <div class="request-view">
    <div class="request-view__pane request-view__pane--top">
    <div class="request-panel__header">
      <div class="request-panel__header-main">
        <p class="request-panel__name">
          {{ request.name }}
          <span class="method-badge method-badge--ws">WS</span>
          <span v-if="dirty" class="request-panel__dirty-dot" title="Unsaved changes"></span>
        </p>
      </div>
      <div class="request-panel__actions">
        <button
          type="button"
          class="button button--ghost"
          title="Discard unsaved edits, back to the last saved version"
          :disabled="!dirty || saving"
          @click="handleRevert"
        >
          Revert
        </button>
        <button type="button" class="button button--secondary" :disabled="!dirty || saving" @click="handleSave">
          {{ saving ? "Saving…" : "Save" }}
        </button>
      </div>
    </div>

    <div class="request-panel__body">
      <p v-if="loading" class="response-pane__hint">Loading request…</p>
      <p v-else-if="loadError" class="response-pane__error">{{ loadError }}</p>

      <template v-else-if="original">
        <p v-if="saveError" class="request-panel__save-error">Save failed: {{ saveError }}</p>

        <div class="request-panel__method-url">
          <span class="ws-panel__protocol-badge">WS</span>
          <input
            v-model="url"
            type="text"
            class="request-panel__url-input"
            placeholder="{{ws_base_url}}/socket"
          />
          <button
            type="button"
            class="button request-panel__send"
            :disabled="connecting || loading"
            @click="handleConnect"
          >
            {{ connecting ? "Connecting…" : "Connect" }}
          </button>
        </div>

        <p class="request-panel__hint-text">
          Only a URL, headers, and a list of text messages to send once connected apply to a
          WebSocket request — no method, params, body, auth, or example response.
        </p>

        <div class="request-panel__tab-panel">
          <h4 class="ws-panel__section-title">Headers</h4>
          <KeyValueEditor v-model="headers" name-placeholder="Header" value-placeholder="Value" mode="headers" />
        </div>

        <div class="request-panel__tab-panel">
          <h4 class="ws-panel__section-title">Messages to send, in order</h4>
          <div class="kv-editor">
            <div v-for="(message, index) in messages" :key="index" class="kv-editor__row ws-panel__message-row">
              <span class="ws-panel__message-index">{{ index + 1 }}</span>
              <input
                class="kv-editor__input"
                type="text"
                placeholder="Message text"
                :value="message"
                @input="updateMessage(index, ($event.target as HTMLInputElement).value)"
              />
              <button
                type="button"
                class="ws-panel__reorder-btn"
                title="Move up"
                :disabled="index === 0"
                @click="moveMessage(index, -1)"
              >
                ▲
              </button>
              <button
                type="button"
                class="ws-panel__reorder-btn"
                title="Move down"
                :disabled="index === messages.length - 1"
                @click="moveMessage(index, 1)"
              >
                ▼
              </button>
              <button type="button" class="kv-editor__remove" title="Remove" @click="removeMessage(index)">
                <Icon name="x" />
              </button>
            </div>
            <button type="button" class="kv-editor__add" @click="addMessage">
              <Icon name="plus" />
              Add message
            </button>
          </div>
        </div>
      </template>
    </div>
    </div>

    <div class="request-view__pane request-view__pane--bottom" style="flex-basis: 320px">
    <div class="response-pane__header">
      <span class="response-pane__header-label">Transcript</span>
    </div>
    <div class="response-pane">
      <p v-if="connecting" class="response-pane__hint">Connecting…</p>
      <p v-else-if="connectError" class="response-pane__error">{{ connectError }}</p>

      <template v-else-if="exchange">
        <div class="response-summary">
          <span class="response-summary__meta"
            >Sent <strong>{{ exchange.sent.length }}</strong></span
          >
          <span class="response-summary__meta"
            >Received <strong>{{ exchange.received.length }}</strong></span
          >
          <span class="response-summary__meta">Time <strong>{{ exchange.elapsed_ms }} ms</strong></span>
        </div>

        <!-- Messages are all sent up front, then whatever comes back is
             collected after — the engine doesn't interleave the two, so the
             transcript is shown as two ordered groups rather than a single
             merged timeline that would misrepresent when things happened. -->
        <p class="ws-panel__transcript-heading">Sent</p>
        <ul v-if="exchange.sent.length > 0" class="ws-panel__transcript">
          <li v-for="(message, index) in exchange.sent" :key="`sent-${index}`" class="ws-panel__transcript-line ws-panel__transcript-line--sent">
            <span class="ws-panel__transcript-arrow">&gt;</span> {{ message }}
          </li>
        </ul>
        <p v-else class="response-pane__hint">No messages were sent.</p>

        <p class="ws-panel__transcript-heading">Received</p>
        <ul v-if="exchange.received.length > 0" class="ws-panel__transcript">
          <li
            v-for="(message, index) in exchange.received"
            :key="`received-${index}`"
            class="ws-panel__transcript-line ws-panel__transcript-line--received"
          >
            <span class="ws-panel__transcript-arrow">&lt;</span> {{ message }}
          </li>
        </ul>
        <p v-else class="response-pane__hint">
          Nothing came back before the connection closed or the read timeout elapsed.
        </p>
      </template>

      <p v-else class="response-pane__hint">Click Connect to open this WebSocket connection.</p>
    </div>
    </div>
  </div>
</template>
