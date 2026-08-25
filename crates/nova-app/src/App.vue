<script setup lang="ts">
import { ref } from "vue";

import { openProject, pickProjectDirectory, validateProject } from "./api/nova";
import type { NovaProject } from "./types/nova";
import Sidebar from "./components/Sidebar.vue";
import ProjectPanel from "./components/ProjectPanel.vue";
import EmptyState from "./components/EmptyState.vue";

const project = ref<NovaProject | null>(null);
const validationIssues = ref<string[]>([]);
const selectedEnvironment = ref<string | null>(null);
const error = ref<string | null>(null);

async function handleOpen() {
  const path = await pickProjectDirectory();
  if (!path) return;

  error.value = null;
  try {
    const [loaded, issues] = await Promise.all([openProject(path), validateProject(path)]);
    project.value = loaded;
    validationIssues.value = issues;
    selectedEnvironment.value =
      loaded.manifest.defaults.environment ?? loaded.environments[0]?.name ?? null;
  } catch (e) {
    project.value = null;
    error.value = String(e);
  }
}
</script>

<template>
  <div class="app-shell">
    <aside class="app-shell__sidebar">
      <Sidebar
        v-if="project"
        :project="project"
        v-model:selected-environment="selectedEnvironment"
      />
    </aside>

    <main class="app-shell__main">
      <ProjectPanel v-if="project" :project="project" :validation-issues="validationIssues" />
      <EmptyState v-else :error="error" @open="handleOpen" />
    </main>
  </div>
</template>
