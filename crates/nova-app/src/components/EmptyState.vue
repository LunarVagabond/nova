<script setup lang="ts">
import Icon from "./Icon.vue";

defineProps<{
  error?: string | null;
}>();

const emit = defineEmits<{
  (e: "open"): void;
  (e: "start-new"): void;
}>();

const PILLARS: { icon: "shield" | "git-branch" | "users" | "code"; label: string }[] = [
  { icon: "shield", label: "Local-first" },
  { icon: "git-branch", label: "Git-native" },
  { icon: "users", label: "Team-friendly" },
  { icon: "code", label: "Open Source" },
];
</script>

<template>
  <div class="empty-state">
    <div class="empty-state__field" aria-hidden="true">
      <div class="empty-state__ring"></div>
    </div>

    <Icon name="sparkle" class="empty-state__mark" />
    <h1 class="empty-state__wordmark">Nova</h1>
    <p class="empty-state__tagline">The API platform for your codebase.</p>

    <ul class="empty-state__pillars">
      <li v-for="pillar in PILLARS" :key="pillar.label" class="empty-state__pillar">
        <Icon :name="pillar.icon" />
        <span>{{ pillar.label }}</span>
      </li>
    </ul>

    <p v-if="error" class="empty-state__error">{{ error }}</p>

    <div class="empty-state__actions">
      <button type="button" class="button empty-state__cta" @click="emit('open')">
        <Icon name="folder" />
        Open a Project
      </button>
      <button
        type="button"
        class="button button--secondary empty-state__cta"
        @click="emit('start-new')"
      >
        <Icon name="plus" />
        Start New Project
      </button>
    </div>

    <p class="empty-state__drop-hint">
      Or drag and drop a <span class="empty-state__accent">nova</span> project folder here
    </p>

    <div class="empty-state__footer">
      <span class="empty-state__footer-rule"></span>
      <Icon name="help-circle" />
      <a
        href="https://github.com/LunarVagabond/nova"
        target="_blank"
        rel="noreferrer"
      >
        Learn more about <span class="empty-state__accent">Nova</span>
      </a>
      <span class="empty-state__footer-rule"></span>
    </div>
  </div>
</template>
