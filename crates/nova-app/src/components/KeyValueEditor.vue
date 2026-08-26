<script setup lang="ts">
// Editable list of name/value rows — shared by the request panel's headers
// and query-param editors. Emits the whole updated array on every change;
// callers own what the array actually means (header vs. query param).
type Row = { name: string; value: string };

const props = defineProps<{
  modelValue: Row[];
  namePlaceholder?: string;
  valuePlaceholder?: string;
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
    <div v-for="(row, index) in modelValue" :key="index" class="kv-editor__row">
      <input
        class="kv-editor__input"
        type="text"
        :placeholder="namePlaceholder ?? 'Name'"
        :value="row.name"
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
        &times;
      </button>
    </div>
    <button type="button" class="kv-editor__add" @click="addRow">+ Add</button>
  </div>
</template>
