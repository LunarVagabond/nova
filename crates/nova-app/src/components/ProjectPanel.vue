<script setup lang="ts">
import type { NovaProject } from "../types/nova";

defineProps<{
  project: NovaProject;
  validationIssues: string[];
}>();
</script>

<template>
  <div>
    <p class="project-panel__meta">{{ project.root }}</p>

    <h2 class="project-panel__section-title">Validation</h2>
    <ul v-if="validationIssues.length > 0" class="validation-issues">
      <li v-for="issue in validationIssues" :key="issue" class="validation-issues__item">
        {{ issue }}
      </li>
    </ul>
    <p v-else class="validation-issues__ok">No issues found.</p>

    <h2 class="project-panel__section-title">Requests</h2>
    <p class="empty-state__hint">
      Browse the collection tree in the sidebar. Request editing and
      execution land here once the engine supports them — for now this view
      proves the GUI is reading the exact same project the CLI does.
    </p>
  </div>
</template>
