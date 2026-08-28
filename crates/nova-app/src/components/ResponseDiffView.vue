<script setup lang="ts">
// Renders a `ResponseDiff` (see #90, `nova_engine::diff`) — status/header/
// body changes between two responses. Purely presentational: the caller
// (`RequestPanel.vue`) decides which two responses are being compared and
// fetches the diff; this component just shows the result.
import type { BodyDiff, ResponseDiff, TextDiffLine } from "../types/nova";

defineProps<{
  diff: ResponseDiff;
}>();

function formatJsonValue(value: unknown): string {
  return JSON.stringify(value);
}

function textLineClass(line: TextDiffLine): string {
  switch (line.kind) {
    case "Added":
      return "response-diff__text-line--added";
    case "Removed":
      return "response-diff__text-line--removed";
    default:
      return "response-diff__text-line--unchanged";
  }
}

function textLinePrefix(line: TextDiffLine): string {
  if (line.kind === "Added") return "+ ";
  if (line.kind === "Removed") return "- ";
  return "  ";
}

function bodyHasChanges(body: BodyDiff): boolean {
  if (body.kind === "Unchanged") return false;
  if (body.kind === "Json") return body.changes.length > 0;
  return body.lines.some((line) => line.kind !== "Unchanged");
}
</script>

<template>
  <div class="response-diff">
    <p v-if="diff.identical" class="response-diff__hint">No differences.</p>

    <template v-else>
      <div v-if="diff.status" class="response-diff__status">
        <span class="response-diff__label">Status</span>
        <span class="response-diff__removed">{{ diff.status.before }}</span>
        <span class="response-diff__arrow">&rarr;</span>
        <span class="response-diff__added">{{ diff.status.after }}</span>
      </div>

      <section v-if="diff.header_changes.length > 0" class="response-diff__section">
        <h4 class="response-diff__section-title">Headers</h4>
        <ul class="response-diff__header-list">
          <li v-for="(change, index) in diff.header_changes" :key="index" class="response-diff__header-row">
            <template v-if="change.kind === 'Added'">
              <span class="response-diff__marker response-diff__marker--added">+</span>
              <span class="response-diff__header-name">{{ change.name }}</span>
              <span class="response-diff__added">{{ change.value }}</span>
            </template>
            <template v-else-if="change.kind === 'Removed'">
              <span class="response-diff__marker response-diff__marker--removed">&minus;</span>
              <span class="response-diff__header-name">{{ change.name }}</span>
              <span class="response-diff__removed">{{ change.value }}</span>
            </template>
            <template v-else>
              <span class="response-diff__marker response-diff__marker--changed">~</span>
              <span class="response-diff__header-name">{{ change.name }}</span>
              <span class="response-diff__removed">{{ change.before }}</span>
              <span class="response-diff__arrow">&rarr;</span>
              <span class="response-diff__added">{{ change.after }}</span>
            </template>
          </li>
        </ul>
      </section>

      <section v-if="bodyHasChanges(diff.body)" class="response-diff__section">
        <h4 class="response-diff__section-title">Body</h4>

        <ul v-if="diff.body.kind === 'Json'" class="response-diff__json-list">
          <li v-for="(change, index) in diff.body.changes" :key="index" class="response-diff__json-row">
            <code class="response-diff__json-path">{{ change.path }}</code>
            <template v-if="change.kind === 'Added'">
              <span class="response-diff__marker response-diff__marker--added">+</span>
              <code class="response-diff__added">{{ formatJsonValue(change.value) }}</code>
            </template>
            <template v-else-if="change.kind === 'Removed'">
              <span class="response-diff__marker response-diff__marker--removed">&minus;</span>
              <code class="response-diff__removed">{{ formatJsonValue(change.value) }}</code>
            </template>
            <template v-else>
              <code class="response-diff__removed">{{ formatJsonValue(change.before) }}</code>
              <span class="response-diff__arrow">&rarr;</span>
              <code class="response-diff__added">{{ formatJsonValue(change.after) }}</code>
            </template>
          </li>
        </ul>

        <div v-else-if="diff.body.kind === 'Text'" class="response-diff__text-diff">
          <div
            v-for="(lineEntry, index) in diff.body.lines"
            :key="index"
            class="response-diff__text-line"
            :class="textLineClass(lineEntry)"
          >{{ textLinePrefix(lineEntry) }}{{ lineEntry.line }}</div>
        </div>
      </section>
    </template>
  </div>
</template>
