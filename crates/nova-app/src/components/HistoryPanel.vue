<script setup lang="ts">
// A read-only view of this project's recent sends (see #81) — a
// master/detail layout: the list on the left is `getHistory`'s summary
// (method/status/timing/timestamp), and clicking a row fetches the full
// request/response via `reopenHistoryEntry` and shows it on the right,
// reusing the same response-pane building blocks `RequestPanel.vue` uses
// for a live send.
//
// History is in-memory and per-session on the engine side (`nova_engine::
// Session::history`) — it resets when the app restarts, and this panel
// doesn't try to make it look otherwise.
import { computed, ref, watch } from "vue";

import { getHistory, reopenHistoryEntry } from "../api/nova";
import type { HistoryDetail, HistorySummary } from "../types/nova";
import { formatBytes, formatTimestamp, methodClass, methodLabel, statusClass } from "../lib/format";
import { languageForContentType } from "../lib/bodyType";
import CodeEditor from "./CodeEditor.vue";
import Icon from "./Icon.vue";

const props = defineProps<{
  projectRoot: string;
  /** Only refetches while true — mirrors `RequestPanel`'s `active` prop, so a hidden panel doesn't poll. */
  active: boolean;
}>();

const entries = ref<HistorySummary[]>([]);
const loading = ref(false);
const loadError = ref<string | null>(null);

const selectedId = ref<number | null>(null);
const detail = ref<HistoryDetail | null>(null);
const detailLoading = ref(false);
const detailError = ref<string | null>(null);

async function refresh() {
  loading.value = true;
  loadError.value = null;
  try {
    entries.value = await getHistory(props.projectRoot);
  } catch (e) {
    loadError.value = String(e);
  } finally {
    loading.value = false;
  }
}

async function selectEntry(entry: HistorySummary) {
  selectedId.value = entry.id;
  detail.value = null;
  detailError.value = null;
  detailLoading.value = true;
  try {
    detail.value = await reopenHistoryEntry(props.projectRoot, entry.id);
  } catch (e) {
    detailError.value = String(e);
  } finally {
    detailLoading.value = false;
  }
}

watch(
  () => props.active,
  (active) => {
    if (active) refresh();
  },
  { immediate: true },
);

function contentTypeOf(headers: { name: string; value: string }[]): string {
  return headers.find((h) => h.name.toLowerCase() === "content-type")?.value ?? "";
}

const requestLanguage = computed(() => languageForContentType(contentTypeOf(detail.value?.request.headers ?? [])));
const responseLanguage = computed(() => languageForContentType(contentTypeOf(detail.value?.response.headers ?? [])));

const responseSize = computed(() =>
  detail.value ? new TextEncoder().encode(detail.value.response.body).length : 0,
);
</script>

<template>
  <div class="history-panel">
    <div class="history-panel__list">
      <div class="history-panel__list-header">
        <h2 class="history-panel__title">History</h2>
        <button type="button" class="icon-button icon-button--outline" title="Refresh" @click="refresh">
          <Icon name="history" />
        </button>
      </div>

      <p v-if="loading" class="history-panel__hint">Loading…</p>
      <p v-else-if="loadError" class="history-panel__error">{{ loadError }}</p>
      <p v-else-if="entries.length === 0" class="history-panel__hint">
        Nothing sent yet this session. Send a request to see it here.
      </p>

      <ul v-else class="history-panel__entries">
        <li v-for="entry in entries" :key="entry.id">
          <button
            type="button"
            class="history-panel__entry"
            :class="{ 'history-panel__entry--active': entry.id === selectedId }"
            @click="selectEntry(entry)"
          >
            <span class="method-badge" :class="methodClass(entry.method)">{{ methodLabel(entry.method) }}</span>
            <span class="history-panel__url" :title="entry.url">{{ entry.url }}</span>
            <span class="response-status history-panel__status" :class="statusClass(entry.status)">
              {{ entry.status }}
            </span>
            <span class="history-panel__meta">{{ entry.elapsed_ms }} ms</span>
            <span class="history-panel__meta">{{ formatTimestamp(entry.sent_at_ms) }}</span>
          </button>
        </li>
      </ul>
    </div>

    <div class="history-panel__detail">
      <p v-if="detailLoading" class="history-panel__hint">Loading…</p>
      <p v-else-if="detailError" class="history-panel__error">{{ detailError }}</p>
      <p v-else-if="!detail" class="history-panel__hint">Select a past send to view it.</p>

      <template v-else>
        <section class="history-panel__section">
          <h3 class="history-panel__section-title">Request</h3>
          <div class="history-panel__request-line">
            <span class="method-badge" :class="methodClass(detail.request.method)">
              {{ methodLabel(detail.request.method) }}
            </span>
            <span class="history-panel__url">{{ detail.request.url }}</span>
          </div>
          <div v-if="detail.request.headers.length > 0" class="history-panel__headers">
            <div v-for="header in detail.request.headers" :key="header.name" class="history-panel__header-row">
              <span class="history-panel__header-name">{{ header.name }}</span>
              <span class="history-panel__header-value">{{ header.value }}</span>
            </div>
          </div>
          <CodeEditor
            v-if="detail.request.body_text.trim().length > 0"
            :model-value="detail.request.body_text"
            :language="requestLanguage"
            readonly
          />
        </section>

        <section class="history-panel__section">
          <h3 class="history-panel__section-title">Response</h3>
          <div class="response-summary">
            <span class="response-status" :class="statusClass(detail.response.status)">
              {{ detail.response.status }}
            </span>
            <span class="response-summary__meta">Time <strong>{{ detail.response.elapsed_ms }} ms</strong></span>
            <span class="response-summary__meta">Size <strong>{{ formatBytes(responseSize) }}</strong></span>
          </div>
          <div v-if="detail.response.headers.length > 0" class="history-panel__headers">
            <div v-for="(header, index) in detail.response.headers" :key="index" class="history-panel__header-row">
              <span class="history-panel__header-name">{{ header.name }}</span>
              <span class="history-panel__header-value">{{ header.value }}</span>
            </div>
          </div>
          <CodeEditor :model-value="detail.response.body" :language="responseLanguage" readonly />
        </section>
      </template>
    </div>
  </div>
</template>
