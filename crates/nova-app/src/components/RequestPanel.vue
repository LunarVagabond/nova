<script setup lang="ts">
import { ref, watch } from "vue";

import { sendRequest } from "../api/nova";
import type { RequestFile, RequestResponse } from "../types/nova";

const props = defineProps<{
  request: RequestFile;
  selectedEnvironment: string | null;
}>();

const sending = ref(false);
const response = ref<RequestResponse | null>(null);
const error = ref<string | null>(null);

// Switching to a different request clears the previous one's response
// rather than leaving a stale one on screen.
watch(
  () => props.request.path,
  () => {
    response.value = null;
    error.value = null;
  },
);

async function handleSend() {
  sending.value = true;
  error.value = null;
  try {
    response.value = await sendRequest(props.request.path, props.selectedEnvironment);
  } catch (e) {
    response.value = null;
    error.value = String(e);
  } finally {
    sending.value = false;
  }
}

function statusClass(status: number): string {
  if (status >= 200 && status < 300) return "response-status--ok";
  if (status >= 400) return "response-status--error";
  return "response-status--other";
}
</script>

<template>
  <div>
    <div class="request-panel__header">
      <div>
        <p class="request-panel__name">{{ request.name }}</p>
        <p class="request-panel__path">{{ request.path }}</p>
      </div>
      <button class="button" :disabled="sending" @click="handleSend">
        {{ sending ? "Sending…" : "Send" }}
      </button>
    </div>

    <div class="response-pane">
      <p v-if="sending" class="response-pane__hint">Sending request…</p>

      <p v-else-if="error" class="response-pane__error">{{ error }}</p>

      <template v-else-if="response">
        <div class="response-summary">
          <span class="response-status" :class="statusClass(response.status)">
            {{ response.status }}
          </span>
          <span class="response-summary__elapsed">{{ response.elapsed_ms }}ms</span>
        </div>

        <h3 class="response-pane__section-title">Headers</h3>
        <ul v-if="response.headers.length > 0" class="response-headers">
          <li v-for="header in response.headers" :key="header.name" class="response-headers__item">
            <span class="response-headers__name">{{ header.name }}</span>
            <span class="response-headers__value">{{ header.value }}</span>
          </li>
        </ul>
        <p v-else class="response-pane__hint">No headers.</p>

        <h3 class="response-pane__section-title">Body</h3>
        <pre v-if="response.body" class="response-body">{{ response.body }}</pre>
        <p v-else class="response-pane__hint">Empty body.</p>
      </template>

      <p v-else class="response-pane__hint">Click Send to execute this request.</p>
    </div>
  </div>
</template>
