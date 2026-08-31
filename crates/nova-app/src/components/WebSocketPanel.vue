<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from "vue";
import type { UnlistenFn } from "@tauri-apps/api/event";

import {
  connectWebSocketSession,
  disconnectWebSocketSession,
  listenForWebSocketSessionClosed,
  listenForWebSocketSessionMessages,
  pickBinaryFrameDestination,
  pickFile,
  readWebSocketRequest,
  saveBinaryFrame,
  saveWebSocketRequest,
  sendWebSocketSessionBinaryFile,
  sendWebSocketSessionMessage,
} from "../api/nova";
import { relativeToRoot } from "../lib/relativePath";
import type { RequestFile, RequestHeader, WebSocketMessage } from "../types/nova";
import { beautifyJson } from "../lib/jsonFormat";
import { formatXml } from "../lib/xmlFormat";
import CodeEditor, { type EditorLanguage } from "./CodeEditor.vue";
import Icon from "./Icon.vue";
import KeyValueEditor from "./KeyValueEditor.vue";

const props = defineProps<{
  request: RequestFile;
  selectedEnvironment: string | null;
  projectRoot: string;
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
// and `dirty` is a plain comparison between the two. `messages` here is the
// request's *saved-messages list* (still exactly `[messages]` on disk) —
// the composer's currently-being-typed text is a separate `composedText`/
// `composedBinaryPath` pair below, not part of this list until explicitly
// saved into it.
const original = ref<{ url: string; headers: RequestHeader[]; messages: WebSocketMessage[] } | null>(
  null,
);

const url = ref("");
const headers = ref<RequestHeader[]>([]);
const messages = ref<WebSocketMessage[]>([]);

const dirty = computed(() => {
  if (!original.value) return false;
  return (
    url.value !== original.value.url ||
    JSON.stringify(headers.value) !== JSON.stringify(original.value.headers) ||
    JSON.stringify(messages.value) !== JSON.stringify(original.value.messages)
  );
});

watch(dirty, (value) => emit("dirtyChange", value));

function applyDraft(draft: { url: string; headers: RequestHeader[]; messages: WebSocketMessage[] }) {
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

// --- Interactive session state -------------------------------------------
//
// One session is open (backend-side) at a time across the whole app — see
// `websocket_session.rs`'s doc comment. Switching tabs doesn't disconnect
// it (the panel just stops being visible), but closing the tab that opened
// it does (see `onBeforeUnmount` below); opening a second WebSocket tab's
// Connect while another is already live surfaces the backend's "already
// open" rejection as `connectError` rather than silently stealing the
// connection.
const connecting = ref(false);
const connectError = ref<string | null>(null);
const sessionConnected = ref(false);

// A transcript entry mirrors whichever of `WebSocketMessage` (sent) or
// `WebSocketReceivedMessage` (received) it came from, flattened for
// display: `text` for a text frame, `binary` for a binary one —
// `dataBase64` is only ever present on a *received* binary frame (so it
// can be saved to disk); a sent binary frame only ever names the file it
// came from, not its bytes, since nova-app never reads those itself.
interface TranscriptEntry {
  direction: "sent" | "received";
  atMs: number;
  text?: string;
  binary?: { len?: number; dataBase64?: string; sourcePath?: string };
}

const transcript = ref<TranscriptEntry[]>([]);

let unlistenMessage: UnlistenFn | null = null;
let unlistenClosed: UnlistenFn | null = null;

async function teardownListeners() {
  if (unlistenMessage) {
    unlistenMessage();
    unlistenMessage = null;
  }
  if (unlistenClosed) {
    unlistenClosed();
    unlistenClosed = null;
  }
}

async function load() {
  loading.value = true;
  loadError.value = null;
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
  // against a stale on-disk URL/headers while the form shows edited fields
  // would be confusing, so save first if there are unsaved edits.
  if (dirty.value) {
    const saved = await handleSave();
    if (!saved) return;
  }

  connecting.value = true;
  connectError.value = null;
  transcript.value = [];

  try {
    unlistenMessage = await listenForWebSocketSessionMessages((message) => {
      const entry: TranscriptEntry =
        message.text !== null
          ? { direction: "received", text: message.text, atMs: message.atMs }
          : {
              direction: "received",
              atMs: message.atMs,
              binary: { len: message.len ?? undefined, dataBase64: message.dataBase64 ?? undefined },
            };
      transcript.value = [...transcript.value, entry];
    });
    unlistenClosed = await listenForWebSocketSessionClosed(() => {
      sessionConnected.value = false;
      teardownListeners();
    });

    await connectWebSocketSession(props.request.path, props.selectedEnvironment);
    sessionConnected.value = true;
  } catch (e) {
    connectError.value = String(e);
    sessionConnected.value = false;
    await teardownListeners();
  } finally {
    connecting.value = false;
  }
}

async function handleDisconnect() {
  try {
    await disconnectWebSocketSession();
  } catch (e) {
    connectError.value = String(e);
  } finally {
    sessionConnected.value = false;
    await teardownListeners();
  }
}

onBeforeUnmount(() => {
  teardownListeners();
  if (sessionConnected.value) {
    disconnectWebSocketSession().catch(() => {
      // Nothing more useful to do with a failed best-effort cleanup on
      // unmount — the panel (and its error display) is already gone.
    });
  }
});

// --- Composer --------------------------------------------------------------
//
// Five message formats the composer offers. "Binary" sends a real binary
// frame sourced from a file on disk (see `nova_engine::WebSocketMessage::
// BinaryFile`) — there's no text to edit for it, just a file picker,
// mirroring `BinaryEditor.vue`'s HTTP body counterpart. The other four are
// all still plain text frames; the format only picks the editor's syntax
// highlighting/beautify, same as before.
type WsMessageFormat = "json" | "text" | "binary" | "xml" | "html";

const WS_FORMAT_OPTIONS: WsMessageFormat[] = ["json", "text", "binary", "xml", "html"];
const WS_FORMAT_LABELS: Record<WsMessageFormat, string> = {
  json: "JSON",
  text: "Text",
  binary: "Binary",
  xml: "XML",
  html: "HTML",
};
const WS_FORMAT_EDITOR_LANGUAGE: Record<WsMessageFormat, EditorLanguage> = {
  json: "json",
  text: "text",
  binary: "text",
  xml: "xml",
  html: "html",
};

const composedText = ref("");
const composedFormat = ref<WsMessageFormat>("json");
// The binary composer's chosen file, project-root-relative — mirrors
// `BinaryEditor.vue`'s `modelValue`. Only meaningful while `composedFormat`
// is "binary".
const composedBinaryPath = ref<string | null>(null);
const composedBinaryError = ref<string | null>(null);
// Index into `messages` this composer's text was loaded from, if any — lets
// Save update that entry in place instead of always appending a new one.
// Cleared whenever the text is edited away from what was loaded, or a
// different saved message is picked, so an unrelated edit can't silently
// clobber a saved entry the user didn't mean to touch.
const loadedMessageIndex = ref<number | null>(null);

const editorLanguage = computed(() => WS_FORMAT_EDITOR_LANGUAGE[composedFormat.value]);
const canBeautify = computed(() => composedFormat.value === "json" || composedFormat.value === "xml");

function beautifyComposedText() {
  if (composedFormat.value === "json") {
    try {
      composedText.value = beautifyJson(composedText.value);
    } catch {
      // Leave it as-is — genuinely invalid JSON has nothing useful for
      // beautify to do with it, same rule `RequestPanel`'s body editor uses.
    }
  } else if (composedFormat.value === "xml") {
    composedText.value = formatXml(composedText.value);
  }
}

async function chooseComposedBinaryFile() {
  composedBinaryError.value = null;
  const picked = await pickFile();
  if (!picked) return;

  const relative = props.projectRoot ? relativeToRoot(props.projectRoot, picked) : null;
  if (relative === null) {
    composedBinaryError.value =
      "That file is outside the project — move or copy it under the project directory first.";
    return;
  }
  composedBinaryPath.value = relative;
}

/** A short label for a saved message with no name of its own — this pass
 * doesn't add a `name:` field to `[messages]` (see the design note in
 * `docs/reference/gui.md`'s WebSocket section for why), so the side panel
 * names each entry by a truncated preview of its own content instead. */
function messagePreview(message: WebSocketMessage): string {
  if (message.kind === "binary_file") return `[binary: ${message.path}]`;
  const trimmed = message.text.trim();
  if (trimmed === "") return "(empty message)";
  const firstLine = trimmed.split("\n", 1)[0];
  return firstLine.length > 48 ? `${firstLine.slice(0, 48)}…` : firstLine;
}

function loadSavedMessage(index: number) {
  const message = messages.value[index];
  loadedMessageIndex.value = index;
  if (!message) return;
  if (message.kind === "binary_file") {
    composedFormat.value = "binary";
    composedBinaryPath.value = message.path;
    composedBinaryError.value = null;
  } else {
    composedFormat.value = composedFormat.value === "binary" ? "json" : composedFormat.value;
    composedText.value = message.text;
  }
}

function saveComposedMessage() {
  const message: WebSocketMessage =
    composedFormat.value === "binary"
      ? { kind: "binary_file", path: composedBinaryPath.value ?? "" }
      : { kind: "text", text: composedText.value };
  if (loadedMessageIndex.value !== null) {
    messages.value = messages.value.map((m, i) => (i === loadedMessageIndex.value ? message : m));
  } else {
    messages.value = [...messages.value, message];
    loadedMessageIndex.value = messages.value.length - 1;
  }
}

function newComposedMessage() {
  composedText.value = "";
  composedFormat.value = "json";
  composedBinaryPath.value = null;
  composedBinaryError.value = null;
  loadedMessageIndex.value = null;
}

function removeSavedMessage(index: number) {
  messages.value = messages.value.filter((_, i) => i !== index);
  if (loadedMessageIndex.value === index) {
    loadedMessageIndex.value = null;
  } else if (loadedMessageIndex.value !== null && loadedMessageIndex.value > index) {
    loadedMessageIndex.value -= 1;
  }
}

const sending = ref(false);

const canSend = computed(() =>
  composedFormat.value === "binary" ? composedBinaryPath.value !== null : composedText.value !== "",
);

async function handleSend() {
  if (!sessionConnected.value || !canSend.value) return;
  sending.value = true;
  try {
    if (composedFormat.value === "binary" && composedBinaryPath.value !== null) {
      await sendWebSocketSessionBinaryFile(composedBinaryPath.value);
      transcript.value = [
        ...transcript.value,
        { direction: "sent", atMs: Date.now(), binary: { sourcePath: composedBinaryPath.value } },
      ];
    } else {
      await sendWebSocketSessionMessage(composedText.value);
      transcript.value = [...transcript.value, { direction: "sent", text: composedText.value, atMs: Date.now() }];
    }
    // Appended immediately rather than waiting on any round-trip — sending
    // doesn't need a reply to know it happened, and the live transcript
    // should reflect that right away.
  } catch (e) {
    connectError.value = String(e);
  } finally {
    sending.value = false;
  }
}

const savingBinaryFrame = ref<number | null>(null);

/** Saves a received binary frame's decoded bytes to a file the user picks. */
async function saveReceivedBinaryFrame(entryIndex: number) {
  const entry = transcript.value[entryIndex];
  if (!entry?.binary?.dataBase64) return;

  savingBinaryFrame.value = entryIndex;
  try {
    const destination = await pickBinaryFrameDestination(`frame-${entry.atMs}.bin`);
    if (!destination) return;
    await saveBinaryFrame(entry.binary.dataBase64, destination);
  } catch (e) {
    connectError.value = String(e);
  } finally {
    savingBinaryFrame.value = null;
  }
}

function formatTimestamp(atMs: number): string {
  return new Date(atMs).toLocaleTimeString([], { hour12: false });
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
          <span class="ws-panel__status" :class="{ 'ws-panel__status--connected': sessionConnected }">
            {{ sessionConnected ? "Connected" : "Disconnected" }}
          </span>
          <button
            v-if="!sessionConnected"
            type="button"
            class="button request-panel__send"
            :disabled="connecting || loading"
            @click="handleConnect"
          >
            {{ connecting ? "Connecting…" : "Connect" }}
          </button>
          <button v-else type="button" class="button button--secondary" @click="handleDisconnect">
            Disconnect
          </button>
        </div>

        <p class="request-panel__hint-text">
          Only a URL and headers apply to a WebSocket request's connection settings — no method,
          params, body, auth, or example response. Compose and send messages once connected below.
        </p>

        <div class="request-panel__tab-panel">
          <h4 class="ws-panel__section-title">Headers</h4>
          <KeyValueEditor v-model="headers" name-placeholder="Header" value-placeholder="Value" mode="headers" />
        </div>

        <div class="request-panel__tab-panel ws-panel__composer">
          <h4 class="ws-panel__section-title">Compose message</h4>
          <div class="ws-panel__composer-layout">
            <div class="ws-panel__composer-main">
              <div class="ws-panel__composer-toolbar">
                <select v-model="composedFormat" class="request-panel__body-type-select">
                  <option v-for="format in WS_FORMAT_OPTIONS" :key="format" :value="format">
                    {{ WS_FORMAT_LABELS[format] }}
                  </option>
                </select>
                <button
                  type="button"
                  class="icon-button icon-button--outline"
                  title="Beautify"
                  :disabled="!canBeautify"
                  @click="beautifyComposedText"
                >
                  <Icon name="wand" />
                </button>
                <button
                  type="button"
                  class="button request-panel__send"
                  :disabled="!sessionConnected || sending || !canSend"
                  @click="handleSend"
                >
                  {{ sending ? "Sending…" : "Send" }}
                </button>
              </div>
              <template v-if="composedFormat === 'binary'">
                <p v-if="composedBinaryError" class="request-panel__save-error">{{ composedBinaryError }}</p>
                <div class="kv-editor">
                  <div class="kv-editor__file-slot">
                    <button type="button" class="kv-editor__file-btn" @click="chooseComposedBinaryFile">
                      <Icon name="file-plus" />
                      {{ composedBinaryPath ? "Change file" : "Choose file" }}
                    </button>
                    <span
                      v-if="composedBinaryPath"
                      class="kv-editor__file-chip"
                      :title="composedBinaryPath"
                      >{{ composedBinaryPath }}</span
                    >
                  </div>
                </div>
                <p class="request-panel__hint-text">
                  Sent as a real binary frame — the file's raw bytes, not typed text.
                </p>
              </template>
              <CodeEditor v-else v-model="composedText" :language="editorLanguage" />
              <div class="ws-panel__composer-save">
                <button type="button" class="button button--ghost" @click="newComposedMessage">New</button>
                <button type="button" class="button button--secondary" @click="saveComposedMessage">
                  {{ loadedMessageIndex !== null ? "Update saved message" : "Save message" }}
                </button>
              </div>
            </div>
            <div class="ws-panel__saved-messages">
              <h5 class="ws-panel__section-title">Saved messages</h5>
              <ul v-if="messages.length > 0" class="ws-panel__saved-list">
                <li
                  v-for="(message, index) in messages"
                  :key="index"
                  class="ws-panel__saved-item"
                  :class="{ 'ws-panel__saved-item--active': loadedMessageIndex === index }"
                >
                  <button type="button" class="ws-panel__saved-item-label" @click="loadSavedMessage(index)">
                    {{ messagePreview(message) }}
                  </button>
                  <button
                    type="button"
                    class="kv-editor__remove"
                    title="Remove this saved message"
                    @click="removeSavedMessage(index)"
                  >
                    <Icon name="x" />
                  </button>
                </li>
              </ul>
              <p v-else class="response-pane__hint">No saved messages yet.</p>
            </div>
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
      <p v-if="connectError" class="response-pane__error">{{ connectError }}</p>

      <ul v-if="transcript.length > 0" class="ws-panel__transcript">
        <li
          v-for="(entry, index) in transcript"
          :key="index"
          class="ws-panel__transcript-line"
          :class="`ws-panel__transcript-line--${entry.direction}`"
        >
          <span class="ws-panel__transcript-arrow">{{ entry.direction === "sent" ? "↑" : "↓" }}</span>
          <span v-if="entry.text !== undefined" class="ws-panel__transcript-text">{{ entry.text }}</span>
          <template v-else-if="entry.binary">
            <span class="ws-panel__transcript-text ws-panel__transcript-text--binary">
              <template v-if="entry.direction === 'sent'">binary file: {{ entry.binary.sourcePath }}</template>
              <template v-else>binary frame: {{ entry.binary.len }} bytes</template>
            </span>
            <button
              v-if="entry.direction === 'received' && entry.binary.dataBase64"
              type="button"
              class="icon-button icon-button--outline"
              title="Save this binary frame to a file"
              :disabled="savingBinaryFrame === index"
              @click="saveReceivedBinaryFrame(index)"
            >
              <Icon name="file-plus" />
            </button>
          </template>
          <span class="ws-panel__transcript-time">{{ formatTimestamp(entry.atMs) }}</span>
        </li>
      </ul>
      <p v-else-if="!connectError" class="response-pane__hint">
        {{ sessionConnected ? "No messages yet — send one above." : "Click Connect to open this WebSocket connection." }}
      </p>
    </div>
    </div>
  </div>
</template>
