<script setup lang="ts">
import type { Collection } from "../types/nova";

defineProps<{
  collection: Collection;
  /** Root collection node (the collections dir itself) renders without its own label. */
  isRoot?: boolean;
}>();
</script>

<template>
  <div class="collection-tree__node">
    <div v-if="!isRoot" class="collection-tree__label">{{ collection.name }}</div>

    <ul class="collection-tree">
      <li v-for="request in collection.requests" :key="request.path">
        <span class="collection-tree__request">{{ request.name }}</span>
      </li>
      <li v-for="child in collection.children" :key="child.path">
        <CollectionNode :collection="child" />
      </li>
    </ul>
  </div>
</template>
