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
  (e: "switchProject"): void;
}>();

function onEnvironmentChange(event: Event) {
  emit("update:selectedEnvironment", (event.target as HTMLSelectElement).value);
}
</script>

<template>
  <div>
    <div class="sidebar-header">
      <div class="sidebar-header__project-row">
        <p class="sidebar-header__project">{{ project.manifest.project.name }}</p>
        <button
          type="button"
          class="sidebar-header__switch"
          title="Switch project"
          @click="emit('switchProject')"
        >
          Switch
        </button>
      </div>

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
