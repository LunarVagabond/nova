<script setup lang="ts">
// Structured per-row editor for a `multipart/form-data` body's fields —
// the Body tab's counterpart to `KeyValueEditor` for Headers/Params, with
// one addition: each row is either a typed-in text value or a file
// attached from disk (a path stored relative to the project root; see
// `nova_engine::MultipartField::file_path`).
import { ref } from "vue";

import { pickFile } from "../api/nova";
import { relativeToRoot } from "../lib/relativePath";
import type { MultipartField } from "../types/nova";
import Icon from "./Icon.vue";

const props = defineProps<{
  modelValue: MultipartField[];
  /** The project's Nova root (`NovaProject.root`) — a chosen file's path is stored relative to this. */
  projectRoot: string;
}>();

const fileOutsideProjectError = ref<string | null>(null);

const emit = defineEmits<{
  (e: "update:modelValue", value: MultipartField[]): void;
}>();

function update(index: number, patch: Partial<MultipartField>) {
  const next = props.modelValue.map((field, i) => (i === index ? { ...field, ...patch } : field));
  emit("update:modelValue", next);
}

function addRow() {
  emit("update:modelValue", [
    ...props.modelValue,
    { name: "", filename: null, content_type: null, value: "", file_path: null },
  ]);
}

function removeRow(index: number) {
  emit(
    "update:modelValue",
    props.modelValue.filter((_, i) => i !== index),
  );
}

function setMode(index: number, mode: "text" | "file") {
  if (mode === "text") {
    update(index, { file_path: null, filename: null });
  } else if (props.modelValue[index].file_path === null) {
    // Switching to File mode with nothing chosen yet still needs an empty
    // value: a file field carries no inline value at all (see
    // `nova_engine::MultipartField`).
    update(index, { value: "" });
  }
}

async function chooseFile(index: number) {
  fileOutsideProjectError.value = null;
  const picked = await pickFile();
  if (!picked) return;

  // A `file_path` is only ever meant to be a reference relative to the
  // project root (the engine refuses anything else at send time) — a file
  // picked from outside the project has nothing sensible to write here,
  // so it's refused here too rather than silently saving a path that
  // won't resolve for anyone else who opens the project.
  const relative = props.projectRoot ? relativeToRoot(props.projectRoot, picked) : null;
  if (relative === null) {
    fileOutsideProjectError.value =
      "That file is outside the project — move or copy it under the project directory first.";
    return;
  }

  const filename = relative.split("/").pop() ?? relative;
  update(index, { file_path: relative, filename, value: "" });
}
</script>

<template>
  <div>
    <p v-if="fileOutsideProjectError" class="request-panel__save-error">{{ fileOutsideProjectError }}</p>
    <div class="kv-editor">
    <div v-for="(field, index) in modelValue" :key="index" class="kv-editor__row">
      <input
        class="kv-editor__input"
        type="text"
        placeholder="name"
        :value="field.name"
        @input="update(index, { name: ($event.target as HTMLInputElement).value })"
      />
      <div class="kv-editor__type-toggle" role="group" aria-label="Field type">
        <button
          type="button"
          class="kv-editor__type-btn"
          :class="{ 'kv-editor__type-btn--active': field.file_path === null }"
          @click="setMode(index, 'text')"
        >
          Text
        </button>
        <button
          type="button"
          class="kv-editor__type-btn"
          :class="{ 'kv-editor__type-btn--active': field.file_path !== null }"
          @click="setMode(index, 'file')"
        >
          File
        </button>
      </div>
      <input
        v-if="field.file_path === null"
        class="kv-editor__input"
        type="text"
        placeholder="value"
        :value="field.value"
        @input="update(index, { value: ($event.target as HTMLInputElement).value })"
      />
      <div v-else class="kv-editor__file-slot">
        <button type="button" class="kv-editor__file-btn" @click="chooseFile(index)">
          <Icon name="file-plus" />
          {{ field.file_path ? "Change file" : "Choose file" }}
        </button>
        <span v-if="field.file_path" class="kv-editor__file-chip" :title="field.file_path">{{
          field.file_path
        }}</span>
      </div>
      <button type="button" class="kv-editor__remove" title="Remove" @click="removeRow(index)">
        <Icon name="x" />
      </button>
    </div>
    <button type="button" class="kv-editor__add" @click="addRow">
      <Icon name="plus" />
      Add field
    </button>
    </div>
  </div>
</template>
