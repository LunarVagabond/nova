<script setup lang="ts">
import type { NovaProject } from "../types/nova";
import CollectionNode from "./CollectionNode.vue";

const props = defineProps<{
  project: NovaProject;
  selectedEnvironment: string | null;
}>();

const emit = defineEmits<{
  (e: "update:selectedEnvironment", value: string): void;
}>();

function onEnvironmentChange(event: Event) {
  emit("update:selectedEnvironment", (event.target as HTMLSelectElement).value);
}
</script>

<template>
  <div>
    <div class="sidebar-header">
      <p class="sidebar-header__project">{{ project.manifest.project.name }}</p>

      <select
        v-if="project.environments.length > 0"
        class="sidebar-header__env-select"
        :value="selectedEnvironment ?? ''"
        @change="onEnvironmentChange"
      >
        <option v-for="env in project.environments" :key="env.name" :value="env.name">
          {{ env.name }}
        </option>
      </select>
    </div>

    <p class="sidebar-section-title">Collections</p>
    <CollectionNode :collection="props.project.collections" is-root />
  </div>
</template>
