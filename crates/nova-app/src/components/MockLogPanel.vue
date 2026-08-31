<script setup lang="ts">
// A read-only, live-updating view of what's actually hitting the running
// mock server (see #159) — method/path/matched-route/status, most recent
// first. Debugging why a client isn't getting the expected canned response
// previously meant digging through terminal output instead of the app
// that started the server.
//
// The call log lives in `nova-app`'s `MockServerState` (Rust), scoped to
// whichever mock server is currently running — it resets whenever the
// server restarts, same as request history resets per session.
import { onBeforeUnmount, ref, watch } from "vue";

import { clearMockCallLog, getMockCallLog } from "../api/nova";
import type { MockCallLogEntry, MockServerStatus } from "../types/nova";
import { formatTimestamp, methodClass, methodLabel, statusClass } from "../lib/format";
import Icon from "./Icon.vue";

const props = defineProps<{
  mockServerStatus: MockServerStatus;
  /** Only polls while true — mirrors `HistoryPanel`/`CookiesPanel`'s `active` prop, so a hidden panel doesn't poll. */
  active: boolean;
}>();

const entries = ref<MockCallLogEntry[]>([]);
const loading = ref(false);
const loadError = ref<string | null>(null);

async function refresh() {
  loadError.value = null;
  try {
    entries.value = await getMockCallLog();
  } catch (e) {
    loadError.value = String(e);
  }
}

async function clearLog() {
  loadError.value = null;
  try {
    await clearMockCallLog();
    entries.value = [];
  } catch (e) {
    loadError.value = String(e);
  }
}

// Polls while the panel is open and the server is running — this is the
// one place in the app a log naturally grows on its own, so a manual
// refresh button alone wouldn't feel "live" the way the ticket asked for.
const POLL_INTERVAL_MS = 1500;
let pollHandle: ReturnType<typeof setInterval> | null = null;

function stopPolling() {
  if (pollHandle !== null) {
    clearInterval(pollHandle);
    pollHandle = null;
  }
}

function startPolling() {
  stopPolling();
  if (props.active && props.mockServerStatus.running) {
    pollHandle = setInterval(refresh, POLL_INTERVAL_MS);
  }
}

watch(
  [() => props.active, () => props.mockServerStatus.running],
  ([active]) => {
    if (active) {
      loading.value = true;
      refresh().finally(() => {
        loading.value = false;
      });
    }
    startPolling();
  },
  { immediate: true },
);

onBeforeUnmount(stopPolling);
</script>

<template>
  <div class="mock-log-panel">
    <div class="mock-log-panel__header">
      <h2 class="mock-log-panel__title">Mock server call log</h2>
      <div class="mock-log-panel__header-actions">
        <button type="button" class="icon-button icon-button--outline" title="Refresh" @click="refresh">
          <Icon name="history" />
        </button>
        <button
          type="button"
          class="button button--danger"
          :disabled="entries.length === 0"
          @click="clearLog"
        >
          Clear
        </button>
      </div>
    </div>

    <p v-if="!mockServerStatus.running" class="mock-log-panel__hint">
      The mock server isn't running. Start it from the top bar to begin logging the requests it
      handles.
    </p>
    <p v-else-if="loading" class="mock-log-panel__hint">Loading…</p>
    <p v-else-if="loadError" class="mock-log-panel__error">{{ loadError }}</p>
    <p v-else-if="entries.length === 0" class="mock-log-panel__hint">
      Nothing has hit the mock server yet. Requests to
      <code>{{ mockServerStatus.host }}:{{ mockServerStatus.port }}</code> will show up here as
      they arrive.
    </p>

    <div v-else class="mock-log-panel__table-wrap">
      <table class="mock-log-panel__table">
        <thead>
          <tr>
            <th>Time</th>
            <th>Method</th>
            <th>Path</th>
            <th>Route</th>
            <th>Status</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="entry in entries" :key="entry.id">
            <td class="mock-log-panel__mono">{{ formatTimestamp(entry.received_at_ms) }}</td>
            <td>
              <span class="method-badge" :class="methodClass(entry.method)">{{ methodLabel(entry.method) }}</span>
            </td>
            <td class="mock-log-panel__mono" :title="entry.path">{{ entry.path }}</td>
            <td class="mock-log-panel__mono">
              <span v-if="entry.matched_route">{{ entry.matched_route }}</span>
              <span v-else class="mock-log-panel__unmatched">no route matched</span>
            </td>
            <td>
              <span class="response-status" :class="statusClass(entry.status)">{{ entry.status }}</span>
            </td>
          </tr>
        </tbody>
      </table>
    </div>
  </div>
</template>
