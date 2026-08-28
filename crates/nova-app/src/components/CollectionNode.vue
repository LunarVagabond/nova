<script setup lang="ts">
import { computed, ref } from "vue";

import { gitStatusKind, type Collection, type GitFileStatus, type GitStatusMap, type RequestFile } from "../types/nova";
import { methodClass, methodLabel } from "../lib/format";
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
  (e: "renameRequest", request: RequestFile): void;
  (e: "duplicateRequest", request: RequestFile): void;
  (e: "deleteRequest", request: RequestFile): void;
}>();

// Non-root collections start expanded; the root's own tree can't collapse
// (it has no label to click to toggle it).
const expanded = ref(true);

function statusFor(path: string): GitFileStatus | null {
  return props.gitStatus?.[path] ?? null;
}

const statusLabels: Record<ReturnType<typeof gitStatusKind>, string> = {
  untracked: "Untracked",
  unstaged: "Modified, not staged",
  staged: "Staged",
  committed: "Committed",
  renamed: "Renamed",
};

function statusTitle(status: GitFileStatus): string {
  if (typeof status !== "string") {
    const fromName = status.renamed.from.split("/").pop() ?? status.renamed.from;
    return `Renamed from ${fromName}`;
  }
  return statusLabels[status];
}

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
            <span
              v-if="statusFor(request.path)"
              class="collection-tree__git-badge"
              :class="`collection-tree__git-badge--${gitStatusKind(statusFor(request.path)!)}`"
              :title="statusTitle(statusFor(request.path)!)"
            ></span>
          </span>
          <span class="collection-tree__actions">
            <button
              type="button"
              class="collection-tree__action"
              title="Rename request"
              @click.stop="emit('renameRequest', request)"
            >
              <Icon name="pencil" />
            </button>
            <button
              type="button"
              class="collection-tree__action"
              title="Duplicate request"
              @click.stop="emit('duplicateRequest', request)"
            >
              <Icon name="copy" />
            </button>
            <button
              type="button"
              class="collection-tree__action collection-tree__action--danger"
              title="Delete request"
              @click.stop="emit('deleteRequest', request)"
            >
              <Icon name="trash" />
            </button>
          </span>
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
          @rename-request="emit('renameRequest', $event)"
          @duplicate-request="emit('duplicateRequest', $event)"
          @delete-request="emit('deleteRequest', $event)"
        />
      </li>
    </ul>
  </div>
</template>
