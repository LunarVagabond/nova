<script setup lang="ts">
// Renders a `ResponseTiming` (#165) as a devtools-style horizontal timeline
// bar — the response pane's Timeline tab. Purely presentational: the caller
// (`RequestPanel.vue`) hands over the timing captured by `nova-engine`'s
// `execute()`.
//
// Only two phases are ever shown, deliberately: `ureq` (the HTTP client
// `nova-engine` uses) doesn't expose DNS lookup / TCP connect / TLS
// handshake as separate, hookable phases, so this doesn't fabricate them.
// "Waiting (TTFB)" bundles DNS + connect + TLS + sending the request +
// waiting on the server into one measured span; "Content download" is the
// time spent reading the response body afterward.
import { computed } from "vue";
import type { ResponseTiming } from "../types/nova";

const props = defineProps<{
  timing: ResponseTiming;
}>();

const totalMs = computed(
  () => props.timing.time_to_first_byte_ms + props.timing.content_download_ms,
);

function percentOf(phaseMs: number): number {
  if (totalMs.value <= 0) return 0;
  return (phaseMs / totalMs.value) * 100;
}
</script>

<template>
  <div class="response-timeline">
    <p class="response-timeline__hint">
      <code>ureq</code> doesn't expose DNS lookup, TCP connect, or TLS handshake as
      separate phases, so those aren't broken out — "Waiting (TTFB)" below covers all of
      that plus sending the request and waiting on the server.
    </p>

    <div class="response-timeline__bar" role="img" :aria-label="`Waiting ${timing.time_to_first_byte_ms} ms, download ${timing.content_download_ms} ms`">
      <div
        class="response-timeline__segment response-timeline__segment--ttfb"
        :style="{ width: `${percentOf(timing.time_to_first_byte_ms)}%` }"
      ></div>
      <div
        class="response-timeline__segment response-timeline__segment--download"
        :style="{ width: `${percentOf(timing.content_download_ms)}%` }"
      ></div>
    </div>

    <ul class="response-timeline__legend">
      <li class="response-timeline__legend-item">
        <span class="response-timeline__swatch response-timeline__swatch--ttfb"></span>
        <span class="response-timeline__legend-label">Waiting (TTFB)</span>
        <span class="response-timeline__legend-value">{{ timing.time_to_first_byte_ms }} ms</span>
      </li>
      <li class="response-timeline__legend-item">
        <span class="response-timeline__swatch response-timeline__swatch--download"></span>
        <span class="response-timeline__legend-label">Content download</span>
        <span class="response-timeline__legend-value">{{ timing.content_download_ms }} ms</span>
      </li>
      <li class="response-timeline__legend-item response-timeline__legend-item--total">
        <span class="response-timeline__legend-label">Total</span>
        <span class="response-timeline__legend-value">{{ totalMs }} ms</span>
      </li>
    </ul>
  </div>
</template>
