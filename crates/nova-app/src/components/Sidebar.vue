<script setup lang="ts">
import { ref } from "vue";

import type { Collection, GitStatusMap, NovaProject, RequestFile } from "../types/nova";
import CollectionNode from "./CollectionNode.vue";
import Icon from "./Icon.vue";

defineProps<{
  project: NovaProject;
  /** The environment currently selected to send requests against — display only here; changed via the top bar's selector. */
  selectedEnvironment: string | null;
  selectedRequestPath?: string | null;
  gitStatus?: GitStatusMap | null;
  /** Whether the project settings/manifest view is the one on screen, to highlight this nav entry. */
  showingProjectSettings?: boolean;
}>();

const emit = defineEmits<{
  (e: "selectRequest", request: RequestFile): void;
  (e: "switchProject"): void;
  (e: "projectSettings"): void;
  (e: "createRequest", collectionPath: string): void;
  (e: "createCollection", collectionPath: string): void;
  (e: "renameCollection", collection: Collection): void;
  (e: "deleteCollection", collection: Collection): void;
  (e: "createEnvironment"): void;
  /** Open the editor for the named environment. */
  (e: "manageEnvironment", name: string): void;
}>();

const collectionsExpanded = ref(true);
const environmentsExpanded = ref(true);
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
          <Icon name="swap" />
        </button>
      </div>
      <button
        type="button"
        class="sidebar-header__settings"
        :class="{ 'sidebar-header__settings--active': showingProjectSettings }"
        @click="emit('projectSettings')"
      >
        <Icon name="settings" />
        Project settings
      </button>
    </div>

    <button
      type="button"
      class="sidebar-section-title sidebar-section-title--collapsible"
      :class="{ 'sidebar-section-title--collapsed': !collectionsExpanded }"
      @click="collectionsExpanded = !collectionsExpanded"
    >
      <Icon name="chevron-down" class="collection-tree__chevron" />
      Collections
    </button>
    <div v-show="collectionsExpanded">
      <CollectionNode
        :collection="project.collections"
        is-root
        :selected-path="selectedRequestPath"
        :git-status="gitStatus"
        @select="emit('selectRequest', $event)"
        @create-request="emit('createRequest', $event)"
        @create-collection="emit('createCollection', $event)"
        @rename-collection="emit('renameCollection', $event)"
        @delete-collection="emit('deleteCollection', $event)"
      />
    </div>

    <div class="sidebar-section-header">
      <button
        type="button"
        class="sidebar-section-title sidebar-section-title--collapsible"
        :class="{ 'sidebar-section-title--collapsed': !environmentsExpanded }"
        @click="environmentsExpanded = !environmentsExpanded"
      >
        <Icon name="chevron-down" class="collection-tree__chevron" />
        Environments
      </button>
      <button
        type="button"
        class="sidebar-section-header__action"
        title="New environment"
        @click="emit('createEnvironment')"
      >
        <Icon name="plus" />
      </button>
    </div>
    <ul v-show="environmentsExpanded" class="environment-list">
      <li v-if="project.environments.length === 0" class="environment-list__empty">
        No environments yet
      </li>
      <li v-for="env in project.environments" :key="env.name">
        <span
          class="environment-list__item"
          title="Edit this environment's variables"
          @click="emit('manageEnvironment', env.name)"
        >
          <span class="environment-list__name">{{ env.name }}</span>
          <Icon
            v-if="env.name === selectedEnvironment"
            name="check"
            class="environment-list__active-mark"
            title="Requests are sent against this environment"
          />
        </span>
      </li>
    </ul>
  </div>
</template>
