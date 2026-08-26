<script setup lang="ts">
import { computed, ref } from "vue";

import type { Collection, GitFileStatus, GitStatusMap, RequestFile } from "../types/nova";
import Icon from "./Icon.vue";

const props = defineProps<{
  collection: Collection;
  /** Root collection node (the collections dir itself) renders without its own label. */
  isRoot?: boolean;
  /** Path of the currently selected request, for highlighting. */
  selectedPath?: string | null;
  /** Per-file git status, keyed by absolute path — `null`/absent means clean or no git repo. */
  gitStatus?: GitStatusMap | null;
}>();

const emit = defineEmits<{
  (e: "select", request: RequestFile): void;
  /** A new request should be created directly inside this collection's directory. */
  (e: "createRequest", collectionPath: string): void;
  /** A new subcollection should be created directly inside this collection's directory. */
  (e: "createCollection", collectionPath: string): void;
  (e: "renameCollection", collection: Collection): void;
  (e: "deleteCollection", collection: Collection): void;
}>();

// Non-root collections start expanded; the root's own tree can't collapse
// (it has no label to click to toggle it).
const expanded = ref(true);

function statusFor(path: string): GitFileStatus | null {
  return props.gitStatus?.[path] ?? null;
}

const statusLabels: Record<GitFileStatus, string> = {
  untracked: "Untracked",
  unstaged: "Modified, not staged",
  staged: "Staged",
  committed: "Committed",
};

// A small dot on the collection's own label when anything underneath it
// (at any depth) has a non-clean git status, so a collapsed/nested
// collection doesn't hide that there's something to see inside it.
const hasDescendantChanges = computed(() => {
  if (!props.gitStatus) return false;
  return subtreeHasChanges(props.collection, props.gitStatus);
});

function subtreeHasChanges(collection: Collection, statuses: GitStatusMap): boolean {
  if (collection.requests.some((r) => statuses[r.path])) return true;
  return collection.children.some((child) => subtreeHasChanges(child, statuses));
}

// Method badges reuse the app's fixed 4-color palette rather than inventing
// per-method colors: GET/safe -> success, POST/primary -> accent,
// PUT+PATCH/modifies -> warning, DELETE -> danger, anything else -> neutral.
const METHOD_LABELS: Record<string, string> = {
  GET: "GET",
  POST: "POST",
  PUT: "PUT",
  PATCH: "PTCH",
  DELETE: "DEL",
  HEAD: "HEAD",
  OPTIONS: "OPT",
};

function methodLabel(method: string): string {
  const upper = method.toUpperCase();
  return METHOD_LABELS[upper] ?? upper.slice(0, 4);
}

function methodClass(method: string): string {
  switch (method.toUpperCase()) {
    case "GET":
      return "method-badge--get";
    case "POST":
      return "method-badge--post";
    case "PUT":
    case "PATCH":
      return "method-badge--modify";
    case "DELETE":
      return "method-badge--delete";
    default:
      return "method-badge--neutral";
  }
}
</script>

<template>
  <div class="collection-tree__node">
    <button
      v-if="!isRoot"
      type="button"
      class="collection-tree__label"
      :class="{ 'collection-tree__label--collapsed': !expanded }"
      @click="expanded = !expanded"
    >
      <span class="collection-tree__label-main">
        <Icon name="chevron-down" class="collection-tree__chevron" />
        <Icon name="folder" class="collection-tree__folder-icon" />
        <span class="collection-tree__label-name">{{ collection.name }}</span>
        <span v-if="hasDescendantChanges" class="collection-tree__git-badge" title="Has uncommitted changes"></span>
      </span>
      <span class="collection-tree__actions">
        <button
          type="button"
          class="collection-tree__action"
          title="New request in this collection"
          @click.stop="emit('createRequest', collection.path)"
        >
          <Icon name="file-plus" />
        </button>
        <button
          type="button"
          class="collection-tree__action"
          title="New subcollection"
          @click.stop="emit('createCollection', collection.path)"
        >
          <Icon name="folder-plus" />
        </button>
        <button
          type="button"
          class="collection-tree__action"
          title="Rename collection"
          @click.stop="emit('renameCollection', collection)"
        >
          <Icon name="pencil" />
        </button>
        <button
          type="button"
          class="collection-tree__action collection-tree__action--danger"
          title="Delete collection"
          @click.stop="emit('deleteCollection', collection)"
        >
          <Icon name="trash" />
        </button>
      </span>
    </button>
    <div v-else class="collection-tree__root-actions">
      <button
        type="button"
        class="collection-tree__root-action"
        title="New request"
        @click="emit('createRequest', collection.path)"
      >
        <Icon name="file-plus" />
      </button>
      <button
        type="button"
        class="collection-tree__root-action"
        title="New collection"
        @click="emit('createCollection', collection.path)"
      >
        <Icon name="folder-plus" />
      </button>
    </div>

    <ul v-show="isRoot || expanded" class="collection-tree">
      <li v-for="request in collection.requests" :key="request.path">
        <span
          class="collection-tree__request"
          :class="{ 'collection-tree__request--selected': request.path === selectedPath }"
          @click="emit('select', request)"
        >
          <span class="collection-tree__request-main">
            <span v-if="request.method" class="method-badge" :class="methodClass(request.method)">
              {{ methodLabel(request.method) }}
            </span>
            <span class="collection-tree__request-name">{{ request.name }}</span>
          </span>
          <span
            v-if="statusFor(request.path)"
            class="collection-tree__git-badge"
            :class="`collection-tree__git-badge--${statusFor(request.path)}`"
            :title="statusLabels[statusFor(request.path)!]"
          ></span>
        </span>
      </li>
      <li v-for="child in collection.children" :key="child.path">
        <CollectionNode
          :collection="child"
          :selected-path="selectedPath"
          :git-status="gitStatus"
          @select="emit('select', $event)"
          @create-request="emit('createRequest', $event)"
          @create-collection="emit('createCollection', $event)"
          @rename-collection="emit('renameCollection', $event)"
          @delete-collection="emit('deleteCollection', $event)"
        />
      </li>
    </ul>
  </div>
</template>
