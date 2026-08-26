<script setup lang="ts">
import { computed, ref, watch } from "vue";

import { saveManifest } from "../api/nova";
import type { Manifest, NovaProject } from "../types/nova";

const props = defineProps<{
  project: NovaProject;
  validationIssues: string[];
}>();

const emit = defineEmits<{
  (e: "dirtyChange", dirty: boolean): void;
  (e: "saved"): void;
}>();

// Editable working copies, split out from `original` rather than editing
// the manifest prop in place, so dirty-state is a plain comparison against
// the last loaded/saved snapshot — same pattern as RequestPanel.vue's
// method/url/query/headers/bodyText.
const original = ref<Manifest>(cloneManifest(props.project.manifest));
const name = ref(original.value.project.name);
const defaultEnvironment = ref(original.value.defaults.environment ?? "");
const timeout = ref(original.value.defaults.timeout ?? "");

function cloneManifest(manifest: Manifest): Manifest {
  return {
    version: manifest.version,
    project: { ...manifest.project },
    defaults: { ...manifest.defaults },
    collections: { ...manifest.collections },
    environments: { ...manifest.environments },
  };
}

function resetFromProject() {
  original.value = cloneManifest(props.project.manifest);
  name.value = original.value.project.name;
  defaultEnvironment.value = original.value.defaults.environment ?? "";
  timeout.value = original.value.defaults.timeout ?? "";
  saveError.value = null;
}

// The manifest editor is a single instance reused across project switches
// (unlike RequestPanel, which remounts per-request via a `:key`), so it
// needs to reset its working copies whenever a different project is
// loaded, not just on first mount.
watch(() => props.project.root, resetFromProject);

const saving = ref(false);
const saveError = ref<string | null>(null);

const dirty = computed(() => {
  return (
    name.value !== original.value.project.name ||
    defaultEnvironment.value !== (original.value.defaults.environment ?? "") ||
    timeout.value !== (original.value.defaults.timeout ?? "")
  );
});

watch(dirty, (value) => emit("dirtyChange", value));

async function handleSave(): Promise<boolean> {
  saving.value = true;
  saveError.value = null;
  try {
    const edited: Manifest = {
      ...original.value,
      project: { ...original.value.project, name: name.value },
      defaults: {
        ...original.value.defaults,
        environment: defaultEnvironment.value.trim() === "" ? null : defaultEnvironment.value,
        timeout: timeout.value.trim() === "" ? null : timeout.value,
      },
    };
    await saveManifest(props.project.root, edited);
    original.value = cloneManifest(edited);
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
    <p class="project-panel__meta">{{ project.root }}</p>

    <h2 class="project-panel__section-title">Manifest</h2>
    <p v-if="saveError" class="request-panel__save-error">Save failed: {{ saveError }}</p>

    <div class="manifest-editor">
      <div class="manifest-editor__field">
        <label class="manifest-editor__label" for="manifest-project-name">Project name</label>
        <input
          id="manifest-project-name"
          v-model="name"
          type="text"
          class="manifest-editor__input"
        />
      </div>

      <div class="manifest-editor__field">
        <label class="manifest-editor__label" for="manifest-default-env">Default environment</label>
        <select id="manifest-default-env" v-model="defaultEnvironment" class="manifest-editor__input">
          <option value="">(none)</option>
          <option v-for="env in project.environments" :key="env.name" :value="env.name">
            {{ env.name }}
          </option>
        </select>
      </div>

      <div class="manifest-editor__field">
        <label class="manifest-editor__label" for="manifest-timeout">Default timeout</label>
        <input
          id="manifest-timeout"
          v-model="timeout"
          type="text"
          class="manifest-editor__input"
          placeholder="e.g. 30s"
        />
      </div>

      <div class="manifest-editor__field">
        <span class="manifest-editor__label">Collections path</span>
        <p class="manifest-editor__readonly">{{ project.manifest.collections.path }}</p>
      </div>

      <div class="manifest-editor__field">
        <span class="manifest-editor__label">Environments path</span>
        <p class="manifest-editor__readonly">{{ project.manifest.environments.path }}</p>
      </div>

      <div class="manifest-editor__actions">
        <button
          type="button"
          class="button button--secondary"
          :disabled="!dirty || saving"
          @click="handleSave"
        >
          {{ saving ? "Saving…" : "Save" }}
        </button>
        <span v-if="dirty" class="request-panel__dirty-dot" title="Unsaved changes"></span>
      </div>
    </div>

    <h2 class="project-panel__section-title">Validation</h2>
    <ul v-if="validationIssues.length > 0" class="validation-issues">
      <li v-for="issue in validationIssues" :key="issue" class="validation-issues__item">
        {{ issue }}
      </li>
    </ul>
    <p v-else class="validation-issues__ok">No issues found.</p>

    <h2 class="project-panel__section-title">Requests</h2>
    <p class="empty-state__hint">
      Select a request in the sidebar to view it and send it against the
      selected environment.
    </p>
  </div>
</template>
