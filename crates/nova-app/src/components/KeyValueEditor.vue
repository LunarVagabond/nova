<script setup lang="ts">
// Editable list of name/value rows — shared by the request panel's headers
// and query-param editors. Emits the whole updated array on every change;
// callers own what the array actually means (header vs. query param).
import { reactive } from "vue";

import Icon from "./Icon.vue";
import VariableAwareInput from "./VariableAwareInput.vue";
import type { ResolvedVariables } from "../types/nova";

// `secret` only means anything when `mode` is `"variables"` — an
// environment variable flagged secret gets its value masked behind a
// reveal toggle in the environment editor (headers/params rows never set
// or read it).
type Row = { name: string; value: string; secret?: boolean };

// Common HTTP header names offered as autocomplete suggestions when
// `mode` is "headers". Not exhaustive — just a reasonable common set;
// any arbitrary custom header name can still be typed freely.
const COMMON_HEADER_NAMES = [
  "Content-Type",
  "Authorization",
  "Accept",
  "Accept-Encoding",
  "Accept-Language",
  "Cache-Control",
  "User-Agent",
  "Cookie",
  "Origin",
  "Referer",
  "X-Requested-With",
  "Content-Length",
  "Host",
  "Connection",
];

const props = defineProps<{
  modelValue: Row[];
  namePlaceholder?: string;
  valuePlaceholder?: string;
  mode?: "headers" | "params" | "variables";
  /**
   * This request's resolved `{{variable}}` values, for the value column's
   * hover tooltips — irrelevant (and not passed) when `mode` is
   * `"variables"`, since there the values themselves *are* the definitions
   * rather than a place a placeholder gets used.
   */
  resolved?: ResolvedVariables | null;
}>();

const emit = defineEmits<{
  (e: "update:modelValue", value: Row[]): void;
}>();

function update(index: number, field: "name" | "value", raw: string) {
  const next = props.modelValue.map((row, i) => (i === index ? { ...row, [field]: raw } : row));
  emit("update:modelValue", next);
}

function addRow() {
  emit("update:modelValue", [...props.modelValue, { name: "", value: "" }]);
}

function removeRow(index: number) {
  emit(
    "update:modelValue",
    props.modelValue.filter((_, i) => i !== index),
  );
}

function toggleSecret(index: number) {
  const next = props.modelValue.map((row, i) =>
    i === index ? { ...row, secret: !row.secret } : row,
  );
  revealed[index] = false;
  emit("update:modelValue", next);
}

// Whether a masked row's value is currently shown in plain text — purely
// local, ephemeral UI state (like any password field's reveal toggle), not
// part of the row data and never persisted.
const revealed = reactive<Record<number, boolean>>({});

function toggleRevealed(index: number) {
  revealed[index] = !revealed[index];
}
</script>

<template>
  <div class="kv-editor">
    <datalist v-if="mode === 'headers'" id="kv-editor-header-names">
      <option v-for="headerName in COMMON_HEADER_NAMES" :key="headerName" :value="headerName" />
    </datalist>
    <div v-for="(row, index) in modelValue" :key="index" class="kv-editor__row">
      <input
        class="kv-editor__input"
        type="text"
        :placeholder="namePlaceholder ?? 'Name'"
        :value="row.name"
        :list="mode === 'headers' ? 'kv-editor-header-names' : undefined"
        @input="update(index, 'name', ($event.target as HTMLInputElement).value)"
      />
      <input
        v-if="mode === 'variables'"
        class="kv-editor__input"
        :type="row.secret && !revealed[index] ? 'password' : 'text'"
        :placeholder="valuePlaceholder ?? 'Value'"
        :value="row.value"
        @input="update(index, 'value', ($event.target as HTMLInputElement).value)"
      />
      <VariableAwareInput
        v-else
        class="kv-editor__input"
        :placeholder="valuePlaceholder ?? 'Value'"
        :model-value="row.value"
        :resolved="resolved"
        @update:model-value="update(index, 'value', $event)"
      />
      <button
        v-if="mode === 'variables' && row.secret"
        type="button"
        class="kv-editor__reveal"
        :title="revealed[index] ? 'Hide value' : 'Reveal value'"
        @click="toggleRevealed(index)"
      >
        <Icon :name="revealed[index] ? 'eye-off' : 'eye'" />
      </button>
      <button
        v-if="mode === 'variables'"
        type="button"
        class="kv-editor__secret-toggle"
        :class="{ 'kv-editor__secret-toggle--active': row.secret }"
        :title="row.secret ? 'Unmark as secret' : 'Mark as secret'"
        @click="toggleSecret(index)"
      >
        <Icon name="lock" />
      </button>
      <button
        type="button"
        class="kv-editor__remove"
        title="Remove"
        @click="removeRow(index)"
      >
        <Icon name="x" />
      </button>
    </div>
    <button type="button" class="kv-editor__add" @click="addRow">
      <Icon name="plus" />
      Add
    </button>
  </div>
</template>
