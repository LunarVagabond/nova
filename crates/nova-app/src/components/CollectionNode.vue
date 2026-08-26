<script setup lang="ts">
import type { Collection, RequestFile } from "../types/nova";

defineProps<{
  collection: Collection;
  /** Root collection node (the collections dir itself) renders without its own label. */
  isRoot?: boolean;
  /** Path of the currently selected request, for highlighting. */
  selectedPath?: string | null;
}>();

const emit = defineEmits<{
  (e: "select", request: RequestFile): void;
  /** A new request should be created directly inside this collection's directory. */
  (e: "createRequest", collectionPath: string): void;
}>();
</script>

<template>
  <div class="collection-tree__node">
    <div v-if="!isRoot" class="collection-tree__label">
      <span class="collection-tree__label-name">{{ collection.name }}</span>
      <button
        type="button"
        class="collection-tree__new-request"
        title="New request in this collection"
        @click="emit('createRequest', collection.path)"
      >
        +
      </button>
    </div>
    <button
      v-else
      type="button"
      class="collection-tree__new-request collection-tree__new-request--root"
      title="New request"
      @click="emit('createRequest', collection.path)"
    >
      + New request
    </button>

    <ul class="collection-tree">
      <li v-for="request in collection.requests" :key="request.path">
        <span
          class="collection-tree__request"
          :class="{ 'collection-tree__request--selected': request.path === selectedPath }"
          @click="emit('select', request)"
        >
          {{ request.name }}
        </span>
      </li>
      <li v-for="child in collection.children" :key="child.path">
        <CollectionNode
          :collection="child"
          :selected-path="selectedPath"
          @select="emit('select', $event)"
          @create-request="emit('createRequest', $event)"
        />
      </li>
    </ul>
  </div>
</template>
