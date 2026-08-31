<script setup lang="ts">
// A viewer/editor for this project's session cookie jar (see #146) —
// `nova_engine::Session` already collects `Set-Cookie` responses and
// replays them on later sends; this panel is the only place in the app
// that shows what's actually stored, lets a single cookie's value be
// edited or the cookie removed, or clears the whole jar at once.
//
// Cookies are in-memory and per-session on the engine side, the same as
// request history (`HistoryPanel.vue`) — they reset when the app restarts,
// and this panel doesn't try to make it look otherwise.
import { ref, watch } from "vue";

import { clearCookies, deleteCookie, getCookies, updateCookie } from "../api/nova";
import type { CookieView } from "../types/nova";
import Icon from "./Icon.vue";
import Modal from "./Modal.vue";

const props = defineProps<{
  projectRoot: string;
  /** Only refetches while true — mirrors `HistoryPanel`'s `active` prop, so a hidden panel doesn't poll. */
  active: boolean;
}>();

const cookies = ref<CookieView[]>([]);
const loading = ref(false);
const loadError = ref<string | null>(null);
const rowError = ref<string | null>(null);

function keyOf(cookie: Pick<CookieView, "host" | "name">): string {
  return `${cookie.host} ${cookie.name}`;
}

async function refresh() {
  loading.value = true;
  loadError.value = null;
  try {
    cookies.value = await getCookies(props.projectRoot);
  } catch (e) {
    loadError.value = String(e);
  } finally {
    loading.value = false;
  }
}

watch(
  () => props.active,
  (active) => {
    if (active) refresh();
  },
  { immediate: true },
);

function formatExpiry(expiresAtMs: number | null): string {
  if (expiresAtMs === null) return "Session";
  return new Date(expiresAtMs).toLocaleString(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  });
}

// Editing is inline, one row at a time — only the value is editable
// (path/domain/secure/expiry describe scope the server set and aren't
// meaningful to hand-edit from here).
const editingKey = ref<string | null>(null);
const editValue = ref("");
const editError = ref<string | null>(null);

function startEdit(cookie: CookieView) {
  editingKey.value = keyOf(cookie);
  editValue.value = cookie.value;
  editError.value = null;
}

function cancelEdit() {
  editingKey.value = null;
  editError.value = null;
}

async function saveEdit(cookie: CookieView) {
  editError.value = null;
  try {
    await updateCookie(props.projectRoot, cookie.host, cookie.name, editValue.value);
    cookie.value = editValue.value;
    editingKey.value = null;
  } catch (e) {
    editError.value = String(e);
  }
}

async function deleteOne(cookie: CookieView) {
  rowError.value = null;
  try {
    await deleteCookie(props.projectRoot, cookie.host, cookie.name);
    cookies.value = cookies.value.filter((c) => keyOf(c) !== keyOf(cookie));
  } catch (e) {
    rowError.value = String(e);
  }
}

const confirmingClearAll = ref(false);
const clearError = ref<string | null>(null);

function askClearAll() {
  clearError.value = null;
  confirmingClearAll.value = true;
}

async function confirmClearAll() {
  clearError.value = null;
  try {
    await clearCookies(props.projectRoot);
    cookies.value = [];
    confirmingClearAll.value = false;
  } catch (e) {
    clearError.value = String(e);
  }
}
</script>

<template>
  <div class="cookies-panel">
    <div class="cookies-panel__header">
      <h2 class="cookies-panel__title">Cookies</h2>
      <div class="cookies-panel__header-actions">
        <button type="button" class="icon-button icon-button--outline" title="Refresh" @click="refresh">
          <Icon name="history" />
        </button>
        <button
          type="button"
          class="button button--danger"
          :disabled="cookies.length === 0"
          @click="askClearAll"
        >
          Clear all
        </button>
      </div>
    </div>

    <p v-if="loading" class="cookies-panel__hint">Loading…</p>
    <p v-else-if="loadError" class="cookies-panel__error">{{ loadError }}</p>
    <p v-else-if="cookies.length === 0" class="cookies-panel__hint">
      No cookies stored yet this session. Cookies collected from a response's
      <code>Set-Cookie</code> header will show up here.
    </p>

    <div v-else class="cookies-panel__table-wrap">
      <p v-if="rowError" class="cookies-panel__error">{{ rowError }}</p>
      <table class="cookies-panel__table">
        <thead>
          <tr>
            <th>Host</th>
            <th>Name</th>
            <th>Value</th>
            <th>Path</th>
            <th>Domain</th>
            <th>Secure</th>
            <th>Expires</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="cookie in cookies" :key="keyOf(cookie)">
            <td class="cookies-panel__mono">{{ cookie.host }}</td>
            <td class="cookies-panel__mono">{{ cookie.name }}</td>
            <td class="cookies-panel__mono cookies-panel__value-cell">
              <template v-if="editingKey === keyOf(cookie)">
                <input
                  v-model="editValue"
                  type="text"
                  class="cookies-panel__value-input"
                  @keydown.enter="saveEdit(cookie)"
                  @keydown.esc="cancelEdit"
                />
                <p v-if="editError" class="cookies-panel__error">{{ editError }}</p>
              </template>
              <span v-else class="cookies-panel__value" :title="cookie.value">{{ cookie.value }}</span>
            </td>
            <td class="cookies-panel__mono">{{ cookie.path }}</td>
            <td class="cookies-panel__mono">{{ cookie.domain ?? "—" }}</td>
            <td>{{ cookie.secure ? "Yes" : "No" }}</td>
            <td>{{ formatExpiry(cookie.expires_at_ms) }}</td>
            <td class="cookies-panel__row-actions">
              <template v-if="editingKey === keyOf(cookie)">
                <button type="button" class="icon-button" title="Save" @click="saveEdit(cookie)">
                  <Icon name="check" />
                </button>
                <button type="button" class="icon-button" title="Cancel" @click="cancelEdit">
                  <Icon name="x" />
                </button>
              </template>
              <template v-else>
                <button type="button" class="icon-button" title="Edit value" @click="startEdit(cookie)">
                  <Icon name="pencil" />
                </button>
                <button type="button" class="icon-button" title="Delete" @click="deleteOne(cookie)">
                  <Icon name="trash" />
                </button>
              </template>
            </td>
          </tr>
        </tbody>
      </table>
    </div>

    <Modal v-if="confirmingClearAll" title="Clear all cookies?" @cancel="confirmingClearAll = false">
      <p>
        Delete every cookie stored for this project's session? This can't be undone, and the
        next request to a site that set one will need to set it again.
      </p>
      <p v-if="clearError" class="modal__error">{{ clearError }}</p>
      <template #actions>
        <button type="button" class="button button--secondary" @click="confirmingClearAll = false">
          Cancel
        </button>
        <button type="button" class="button button--danger" @click="confirmClearAll">Clear all</button>
      </template>
    </Modal>
  </div>
</template>
