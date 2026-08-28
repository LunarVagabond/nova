<script setup lang="ts">
// The global application header: sits above the sidebar and the main
// workspace alike (see `.topbar` spanning both grid columns in
// `_topbar.scss`), and owns app-wide identity/actions — which project is
// open, which environment requests are sent against, and project settings —
// so the sidebar underneath it can stay a pure navigation tree.
import Icon from "./Icon.vue";
import type { MockServerStatus } from "../types/nova";

defineProps<{
  projectName: string | null;
  environments: { name: string }[];
  selectedEnvironment: string | null;
  showingProjectSettings: boolean;
  showingHistory: boolean;
  runningTests: boolean;
  mockServerStatus: MockServerStatus;
  mockServerBusy: boolean;
}>();

const emit = defineEmits<{
  (e: "update:selectedEnvironment", value: string): void;
  (e: "switchProject"): void;
  (e: "projectSettings"): void;
  (e: "showHistory"): void;
  (e: "runTests"): void;
  (e: "importExport"): void;
  (e: "toggleMockServer"): void;
}>();
</script>

<template>
  <header class="topbar">
    <div class="topbar__brand">
      <span class="topbar__mark">N</span>
      <span class="topbar__wordmark">Nova</span>
    </div>

    <template v-if="projectName">
      <span class="topbar__sep">/</span>
      <span class="topbar__project" :title="projectName">{{ projectName }}</span>
      <button
        type="button"
        class="icon-button icon-button--outline"
        title="Switch project"
        @click="emit('switchProject')"
      >
        <Icon name="swap" />
      </button>
    </template>

    <div class="topbar__spacer"></div>

    <select
      v-if="environments.length > 0"
      class="topbar__env-select"
      :value="selectedEnvironment"
      title="Environment requests are sent against"
      @change="emit('update:selectedEnvironment', ($event.target as HTMLSelectElement).value)"
    >
      <option v-for="env in environments" :key="env.name" :value="env.name">
        {{ env.name }}
      </option>
    </select>

    <button
      v-if="projectName"
      type="button"
      class="mock-toggle"
      :class="{ 'mock-toggle--running': mockServerStatus.running }"
      :disabled="mockServerBusy"
      :title="
        mockServerStatus.running
          ? `Mock server running on ${mockServerStatus.host}:${mockServerStatus.port} — click to stop`
          : 'Mock server is off — click to start it for this project'
      "
      @click="emit('toggleMockServer')"
    >
      <span class="mock-toggle__dot"></span>
      <Icon name="server" />
      <span class="mock-toggle__label">
        {{
          mockServerStatus.running
            ? `${mockServerStatus.host}:${mockServerStatus.port}`
            : "Mock server"
        }}
      </span>
    </button>

    <button
      v-if="projectName"
      type="button"
      class="icon-button icon-button--outline"
      title="Run tests for the whole project"
      :disabled="runningTests"
      @click="emit('runTests')"
    >
      <Icon name="play" />
    </button>

    <button
      v-if="projectName"
      type="button"
      class="icon-button icon-button--outline"
      :class="{ 'icon-button--active': showingHistory }"
      title="Request history"
      @click="emit('showHistory')"
    >
      <Icon name="history" />
    </button>

    <button
      v-if="projectName"
      type="button"
      class="icon-button icon-button--outline"
      title="Import / export"
      @click="emit('importExport')"
    >
      <Icon name="transfer" />
    </button>

    <button
      v-if="projectName"
      type="button"
      class="icon-button icon-button--outline"
      :class="{ 'icon-button--active': showingProjectSettings }"
      title="Project settings"
      @click="emit('projectSettings')"
    >
      <Icon name="settings" />
    </button>
  </header>
</template>
