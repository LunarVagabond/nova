<script setup lang="ts">
// The Body tab's editor for a "binary" body — a single file's raw bytes
// sent as the entire request payload (see `nova_engine::RequestBody::Binary`),
// as opposed to `MultipartEditor.vue`, where a file is one part among
// possibly several. Just a file picker: there's no per-field structure to
// edit, only which file.
import { ref } from "vue";

import { pickFile } from "../api/nova";
import { relativeToRoot } from "../lib/relativePath";
import Icon from "./Icon.vue";

const props = defineProps<{
  /** The binary body's file path, relative to the project root — `null` if none chosen yet. */
  modelValue: string | null;
  /** The project's Nova root (`NovaProject.root`) — a chosen file's path is stored relative to this. */
  projectRoot: string;
}>();

const emit = defineEmits<{
  (e: "update:modelValue", value: string | null): void;
}>();

const fileOutsideProjectError = ref<string | null>(null);

async function chooseFile() {
  fileOutsideProjectError.value = null;
  const picked = await pickFile();
  if (!picked) return;

  // A binary body's file path is only ever meant to be a reference
  // relative to the project root (the engine refuses anything else at
  // send time) — a file picked from outside the project has nothing
  // sensible to write here, so it's refused here too rather than silently
  // saving a path that won't resolve for anyone else who opens the
  // project.
  const relative = props.projectRoot ? relativeToRoot(props.projectRoot, picked) : null;
  if (relative === null) {
    fileOutsideProjectError.value =
      "That file is outside the project — move or copy it under the project directory first.";
    return;
  }

  emit("update:modelValue", relative);
}
</script>

<template>
  <div>
    <p v-if="fileOutsideProjectError" class="request-panel__save-error">{{ fileOutsideProjectError }}</p>
    <div class="kv-editor">
      <div class="kv-editor__file-slot">
        <button type="button" class="kv-editor__file-btn" @click="chooseFile">
          <Icon name="file-plus" />
          {{ modelValue ? "Change file" : "Choose file" }}
        </button>
        <span v-if="modelValue" class="kv-editor__file-chip" :title="modelValue">{{ modelValue }}</span>
      </div>
    </div>
  </div>
</template>
