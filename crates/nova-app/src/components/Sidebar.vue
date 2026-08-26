<script setup lang="ts">
import type { NovaProject, RequestFile } from "../types/nova";
import CollectionNode from "./CollectionNode.vue";

const props = defineProps<{
  project: NovaProject;
  selectedEnvironment: string | null;
  selectedRequestPath?: string | null;
}>();

const emit = defineEmits<{
  (e: "update:selectedEnvironment", value: string): void;
  (e: "selectRequest", request: RequestFile): void;
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
    <CollectionNode
      :collection="props.project.collections"
      is-root
      :selected-path="selectedRequestPath"
      @select="emit('selectRequest', $event)"
    />
  </div>
</template>
