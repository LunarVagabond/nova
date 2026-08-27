<script setup lang="ts">
// Editable list of name/value rows — shared by the request panel's headers
// and query-param editors. Emits the whole updated array on every change;
// callers own what the array actually means (header vs. query param).
import Icon from "./Icon.vue";

type Row = { name: string; value: string };

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
  mode?: "headers" | "params";
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
        class="kv-editor__input"
        type="text"
        :placeholder="valuePlaceholder ?? 'Value'"
        :value="row.value"
        @input="update(index, 'value', ($event.target as HTMLInputElement).value)"
      />
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
