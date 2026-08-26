<script setup lang="ts">
import type { Collection, NovaProject, RequestFile } from "../types/nova";
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
  (e: "createRequest", collectionPath: string): void;
  (e: "createCollection", collectionPath: string): void;
  (e: "renameCollection", collection: Collection): void;
  (e: "deleteCollection", collection: Collection): void;
  (e: "createEnvironment"): void;
  /** Open the editor for the named environment (the selected one by default). */
  (e: "manageEnvironment", name: string): void;
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

      <div class="sidebar-header__env-row">
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
        <p v-else class="sidebar-header__env-empty">No environments yet</p>

        <button
          v-if="selectedEnvironment"
          type="button"
          class="sidebar-header__env-action"
          title="Edit this environment's variables"
          @click="emit('manageEnvironment', selectedEnvironment)"
        >
          ✎
        </button>
        <button
          type="button"
          class="sidebar-header__env-action"
          title="New environment"
          @click="emit('createEnvironment')"
        >
          +
        </button>
      </div>
    </div>

    <p class="sidebar-section-title">Collections</p>
    <CollectionNode
      :collection="props.project.collections"
      is-root
      :selected-path="selectedRequestPath"
      @select="emit('selectRequest', $event)"
      @create-request="emit('createRequest', $event)"
      @create-collection="emit('createCollection', $event)"
      @rename-collection="emit('renameCollection', $event)"
      @delete-collection="emit('deleteCollection', $event)"
    />
  </div>
</template>
