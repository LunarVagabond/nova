<script setup lang="ts">
// A custom in-app dialog, used in place of `window.prompt`/`window.confirm`
// — those are unreliable inside Tauri's webview (unimplemented on some
// platforms, and can hang or crash the window when called), and they can't
// be styled to match the rest of the app anyway.
withDefaults(
  defineProps<{
    title: string;
    /** Wider dialog for content that doesn't fit the default 28rem width — e.g. a test-results breakdown. */
    wide?: boolean;
  }>(),
  { wide: false },
);

const emit = defineEmits<{
  cancel: [];
}>();

function onOverlayClick() {
  emit("cancel");
}
</script>

<template>
  <div class="modal-overlay" @mousedown.self="onOverlayClick">
    <div
      class="modal"
      :class="{ 'modal--wide': wide }"
      role="dialog"
      aria-modal="true"
      @keydown.esc="emit('cancel')"
    >
      <h2 class="modal__title">{{ title }}</h2>
      <div class="modal__body">
        <slot />
      </div>
      <div class="modal__actions">
        <slot name="actions" />
      </div>
    </div>
  </div>
</template>
