<script setup lang="ts">
defineProps<{
  error?: string | null;
}>();

const emit = defineEmits<{
  (e: "open"): void;
}>();
</script>

<template>
  <div class="empty-state">
    <p class="empty-state__title">No project open</p>
    <p class="empty-state__hint">
      Open a directory containing a <code>nova/nova.yaml</code> manifest, or a
      repository that contains one, to browse its environments and collections.
    </p>
    <p v-if="error" class="empty-state__error">{{ error }}</p>
    <button class="button" @click="emit('open')">Open Project</button>
  </div>
</template>
