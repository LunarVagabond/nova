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
}>();
</script>

<template>
  <div class="collection-tree__node">
    <div v-if="!isRoot" class="collection-tree__label">{{ collection.name }}</div>

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
        />
      </li>
    </ul>
  </div>
</template>
