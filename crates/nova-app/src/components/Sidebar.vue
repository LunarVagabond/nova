<script setup lang="ts">
import { ref, watch } from "vue";

import type { Collection, GitStatusMap, NovaProject, RequestFile } from "../types/nova";
import CollectionNode from "./CollectionNode.vue";
import Icon from "./Icon.vue";

defineProps<{
  project: NovaProject;
  /** The environment currently selected to send requests against — display only here; changed via the top bar's selector. */
  selectedEnvironment: string | null;
  selectedRequestPath?: string | null;
  gitStatus?: GitStatusMap | null;
}>();

const emit = defineEmits<{
  (e: "selectRequest", request: RequestFile): void;
  (e: "createRequest", collectionPath: string): void;
  (e: "createCollection", collectionPath: string): void;
  (e: "renameCollection", collection: Collection): void;
  (e: "deleteCollection", collection: Collection): void;
  (e: "renameRequest", request: RequestFile): void;
  (e: "duplicateRequest", request: RequestFile): void;
  (e: "deleteRequest", request: RequestFile): void;
  (e: "createEnvironment"): void;
  /** Open the editor for the named environment. */
  (e: "manageEnvironment", name: string): void;
}>();

// Persisted so the sidebar's accordion sections don't reset to expanded
// every time the app restarts.
const COLLECTIONS_EXPANDED_KEY = "nova.sidebarCollectionsExpanded";
const ENVIRONMENTS_EXPANDED_KEY = "nova.sidebarEnvironmentsExpanded";

function loadExpanded(key: string): boolean {
  const stored = localStorage.getItem(key);
  return stored === null ? true : stored === "true";
}

const collectionsExpanded = ref(loadExpanded(COLLECTIONS_EXPANDED_KEY));
const environmentsExpanded = ref(loadExpanded(ENVIRONMENTS_EXPANDED_KEY));

watch(collectionsExpanded, (value) => localStorage.setItem(COLLECTIONS_EXPANDED_KEY, String(value)));
watch(environmentsExpanded, (value) => localStorage.setItem(ENVIRONMENTS_EXPANDED_KEY, String(value)));

const filterQuery = ref("");
watch(filterQuery, (value) => {
  // Otherwise typing into the filter while the Collections section happens
  // to be collapsed would silently show nothing.
  if (value) collectionsExpanded.value = true;
});
</script>

<template>
  <div>
    <div class="sidebar-search">
      <input
        v-model="filterQuery"
        type="search"
        class="sidebar-search__input"
        placeholder="Filter requests…"
      />
    </div>

    <button
      type="button"
      class="sidebar-section-title sidebar-section-title--collapsible"
      :class="{ 'sidebar-section-title--collapsed': !collectionsExpanded }"
      @click="collectionsExpanded = !collectionsExpanded"
    >
      <span class="collection-tree__chevron-box">
        <Icon name="chevron-down" class="collection-tree__chevron" />
      </span>
      Collections
    </button>
    <div v-show="collectionsExpanded">
      <CollectionNode
        :collection="project.collections"
        is-root
        :selected-path="selectedRequestPath"
        :git-status="gitStatus"
        :filter="filterQuery"
        @select="emit('selectRequest', $event)"
        @create-request="emit('createRequest', $event)"
        @create-collection="emit('createCollection', $event)"
        @rename-collection="emit('renameCollection', $event)"
        @delete-collection="emit('deleteCollection', $event)"
        @rename-request="emit('renameRequest', $event)"
        @duplicate-request="emit('duplicateRequest', $event)"
        @delete-request="emit('deleteRequest', $event)"
      />
    </div>

    <div class="sidebar-section-header">
      <button
        type="button"
        class="sidebar-section-title sidebar-section-title--collapsible"
        :class="{ 'sidebar-section-title--collapsed': !environmentsExpanded }"
        @click="environmentsExpanded = !environmentsExpanded"
      >
        <span class="collection-tree__chevron-box">
          <Icon name="chevron-down" class="collection-tree__chevron" />
        </span>
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
