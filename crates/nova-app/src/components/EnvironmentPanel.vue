<script setup lang="ts">
import { computed, ref, watch } from "vue";

import { saveEnvironment } from "../api/nova";
import type { AuthScheme, NovaEnvironment } from "../types/nova";
import AuthEditor from "./AuthEditor.vue";
import KeyValueEditor from "./KeyValueEditor.vue";

const props = defineProps<{
  environment: NovaEnvironment;
  projectRoot: string;
}>();

const emit = defineEmits<{
  (e: "dirtyChange", dirty: boolean): void;
  (e: "saved"): void;
  (e: "delete", environment: NovaEnvironment): void;
}>();

type Row = { name: string; value: string };

function recordToRows(record: Record<string, string>): Row[] {
  return Object.entries(record).map(([name, value]) => ({ name, value }));
}

function rowsToRecord(rows: Row[]): Record<string, string> {
  const record: Record<string, string> = {};
  for (const row of rows) {
    if (row.name.trim() === "") continue;
    record[row.name] = row.value;
  }
  return record;
}

function cloneEnvironment(environment: NovaEnvironment): NovaEnvironment {
  return {
    name: environment.name,
    variables: { ...environment.variables },
    auth: environment.auth ? { ...environment.auth } : null,
    path: environment.path,
  };
}

// Editable working copies, split out from `original` rather than editing
// the environment prop in place, so dirty-state is a plain comparison
// against the last loaded/saved snapshot — same pattern as
// RequestPanel.vue/ProjectPanel.vue.
const original = ref<NovaEnvironment>(cloneEnvironment(props.environment));
const name = ref(original.value.name);
const variables = ref<Row[]>(recordToRows(original.value.variables));
const auth = ref<AuthScheme | null>(original.value.auth ? { ...original.value.auth } : null);

const saving = ref(false);
const saveError = ref<string | null>(null);

function resetFromEnvironment() {
  original.value = cloneEnvironment(props.environment);
  name.value = original.value.name;
  variables.value = recordToRows(original.value.variables);
  auth.value = original.value.auth ? { ...original.value.auth } : null;
  saveError.value = null;
}

// This panel is a single instance reused across environment switches
// (picking a different one to manage), not remounted per environment, so
// it needs to reset its working copies whenever a different environment
// is opened for editing.
watch(() => props.environment.path, resetFromEnvironment);

const dirty = computed(() => {
  return (
    name.value !== original.value.name ||
    JSON.stringify(rowsToRecord(variables.value)) !== JSON.stringify(original.value.variables) ||
    JSON.stringify(auth.value) !== JSON.stringify(original.value.auth)
  );
});

watch(dirty, (value) => emit("dirtyChange", value));

async function handleSave(): Promise<boolean> {
  saving.value = true;
  saveError.value = null;
  try {
    const variableRecord = rowsToRecord(variables.value);
    const authScheme = auth.value ? { ...auth.value } : null;
    await saveEnvironment(props.projectRoot, original.value.path, original.value.name, {
      name: name.value,
      variables: variableRecord,
      auth: authScheme,
    });
    original.value = {
      ...original.value,
      name: name.value,
      variables: variableRecord,
      auth: authScheme,
    };
    emit("saved");
    return true;
  } catch (e) {
    saveError.value = String(e);
    return false;
  } finally {
    saving.value = false;
  }
}

defineExpose({ dirty, save: handleSave });
</script>

<template>
  <div>
    <div class="request-panel__header">
      <div>
        <p class="request-panel__name">
          {{ original.name }}
          <span v-if="dirty" class="request-panel__dirty-dot" title="Unsaved changes"></span>
        </p>
        <p class="request-panel__path">{{ original.path }}</p>
      </div>
      <div class="request-panel__actions">
        <button
          type="button"
          class="button button--danger"
          title="Delete this environment"
          @click="emit('delete', original)"
        >
          Delete
        </button>
        <button
          type="button"
          class="button button--secondary"
          :disabled="!dirty || saving"
          @click="handleSave"
        >
          {{ saving ? "Saving…" : "Save" }}
        </button>
      </div>
    </div>

    <p v-if="saveError" class="request-panel__save-error">Save failed: {{ saveError }}</p>

    <div class="manifest-editor">
      <div class="manifest-editor__field">
        <label class="manifest-editor__label" for="environment-name">Name</label>
        <input id="environment-name" v-model="name" type="text" class="manifest-editor__input" />
      </div>
    </div>

    <h2 class="project-panel__section-title">Variables</h2>
    <KeyValueEditor v-model="variables" name-placeholder="Variable" value-placeholder="Value" />

    <h2 class="project-panel__section-title">Default auth</h2>
    <p class="request-panel__hint-text">
      Applied to every request resolved against this environment, unless the request sets up
      auth of its own.
    </p>
    <AuthEditor v-model="auth" id-prefix="environment-auth" />
  </div>
</template>
