<script setup lang="ts">
// A single-line field that colors `{{variable}}` placeholders and shows
// what each one resolves to on hover — everywhere a request's URL, header,
// or param value is typed (a native form field can't color part of its own
// text, so this overlays a non-interactive, identically-styled backdrop
// rendering the same text with placeholder spans, on top of a
// transparent-text real field that still owns focus/caret/selection).
//
// Deliberately a `<textarea>`, not an `<input>`, even though this is always
// single-line content: an `<input>`'s text is vertically centered by an
// internal UA algorithm that ignores `line-height`, which doesn't line up
// with a plain `<div>` backdrop's ordinary text layout no matter how
// carefully padding/line-height are matched — the visible misalignment
// that comes from that mismatch is exactly what this component exists to
// avoid. A `<textarea>` lays out text the same way a `<div>` does, so it
// stays pixel-aligned with the backdrop; `rows`/`wrap`/the keydown handler
// below keep it behaving like a single-line field despite the element.
//
// `resolved` is the request panel's already-loaded variables map (or
// `null`/`undefined` while it hasn't loaded yet, or for a value that can't
// contain variables) — see `lib/variableTokens.ts` for how a token's
// tooltip text is derived from it.
import { computed, nextTick, ref } from "vue";
import { describeVariable, findVariableTokens } from "../lib/variableTokens";
import type { ResolvedVariables } from "../types/nova";

const props = defineProps<{
  modelValue: string;
  resolved?: ResolvedVariables | null;
  placeholder?: string;
}>();

const emit = defineEmits<{
  (e: "update:modelValue", value: string): void;
}>();

const fieldEl = ref<HTMLTextAreaElement | null>(null);
const backdrop = ref<HTMLDivElement | null>(null);

type Segment = { text: string; isVariable: boolean; tooltip?: string };

const segments = computed<Segment[]>(() => {
  const text = props.modelValue;
  const tokens = findVariableTokens(text);
  if (tokens.length === 0) return [{ text, isVariable: false }];

  const result: Segment[] = [];
  let cursor = 0;
  for (const token of tokens) {
    if (token.start > cursor) result.push({ text: text.slice(cursor, token.start), isVariable: false });
    const { text: tooltip } = describeVariable(token.name, props.resolved);
    result.push({ text: text.slice(token.start, token.end), isVariable: true, tooltip });
    cursor = token.end;
  }
  if (cursor < text.length) result.push({ text: text.slice(cursor), isVariable: false });
  return result;
});

function onInput(event: Event) {
  // Belt-and-suspenders against a newline sneaking in via paste — Enter
  // itself never inserts one, see `preventNewline` below.
  emit("update:modelValue", (event.target as HTMLTextAreaElement).value.replace(/[\r\n]+/g, ""));
}

function preventNewline(event: KeyboardEvent) {
  if (event.key === "Enter") event.preventDefault();
}

// Keeps the backdrop's text aligned under the real field while it's
// scrolled horizontally (a value longer than the field is wide).
function syncScroll() {
  if (fieldEl.value && backdrop.value) {
    backdrop.value.scrollLeft = fieldEl.value.scrollLeft;
  }
}

// Hovering a placeholder to show its tooltip can't be done with CSS
// `:hover`/a native `title` attribute on the backdrop's spans — the real
// field sits on top to keep receiving clicks/typing, and `:hover` only
// ever applies to the one topmost hit-tested element (plus its ancestors),
// never a sibling underneath it, no matter what that sibling's own
// `pointer-events` says. `elementsFromPoint` instead returns every
// hit-testable element at a point regardless of stacking order, so it
// finds a token span even though the field is drawn over it (the span
// needs its own `pointer-events: auto` to be hit-testable at all, since it
// sits inside the backdrop's `pointer-events: none` container).
//
// The tooltip itself is teleported to `<body>` and positioned with
// `position: fixed` in viewport coordinates — this field can sit inside any
// number of scrolling/clipping panes (the response pane, a tab panel, the
// variables drawer…), and a tooltip positioned/clipped relative to one of
// those would either get cut off or need every ancestor individually
// special-cased to raise its z-index. Rendering at the document root next
// to every modal/dropdown sidesteps both problems at once.
const tooltipEl = ref<HTMLDivElement | null>(null);
const tooltip = ref<{ text: string; left: number; top: number } | null>(null);

async function onMouseMove(event: MouseEvent) {
  const hit = document
    .elementsFromPoint(event.clientX, event.clientY)
    .find((el): el is HTMLElement => el instanceof HTMLElement && el.classList.contains("var-input__token"));
  const text = hit?.dataset.tooltip;
  if (!hit || !text) {
    tooltip.value = null;
    return;
  }

  const hitRect = hit.getBoundingClientRect();
  // Provisional placement just below-left of the token; corrected below
  // once the tooltip's actual size is known (its text length varies, so
  // size can't be predicted before it's rendered).
  tooltip.value = { text, left: hitRect.left, top: hitRect.bottom + 4 };

  await nextTick();
  const el = tooltipEl.value;
  if (!el || !tooltip.value) return;
  const rect = el.getBoundingClientRect();
  const margin = 8;

  let left = Math.min(tooltip.value.left, window.innerWidth - rect.width - margin);
  left = Math.max(margin, left);

  // Flips above the token instead of below whenever below would run off
  // the bottom of the viewport (e.g. a field near the bottom of the
  // response pane) — a fixed offset from the token rather than a hardcoded
  // pixel guess, so it still tucks in close regardless of tooltip height.
  let top = tooltip.value.top;
  if (top + rect.height > window.innerHeight - margin) {
    top = hitRect.top - rect.height - 4;
  }
  top = Math.max(margin, top);

  tooltip.value = { text, left, top };
}

function onMouseLeave() {
  tooltip.value = null;
}
</script>

<template>
  <div class="var-input" @mousemove="onMouseMove" @mouseleave="onMouseLeave">
    <div ref="backdrop" class="var-input__backdrop" aria-hidden="true">
      <span
        v-for="(segment, index) in segments"
        :key="index"
        :class="{ 'var-input__token': segment.isVariable }"
        :data-tooltip="segment.tooltip"
        >{{ segment.text }}</span
      >
    </div>
    <textarea
      ref="fieldEl"
      class="var-input__field"
      rows="1"
      wrap="off"
      :placeholder="placeholder"
      :value="modelValue"
      autocomplete="off"
      autocorrect="off"
      autocapitalize="off"
      spellcheck="false"
      @input="onInput"
      @keydown="preventNewline"
      @scroll="syncScroll"
    ></textarea>
    <Teleport to="body">
      <div
        v-if="tooltip"
        ref="tooltipEl"
        class="var-input__tooltip"
        :style="{ left: `${tooltip.left}px`, top: `${tooltip.top}px` }"
      >
        {{ tooltip.text }}
      </div>
    </Teleport>
  </div>
</template>
