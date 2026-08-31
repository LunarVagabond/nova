<script setup lang="ts">
// A syntax-highlighted, lint-aware code editor for the request body field
// (JSON/XML today; a plain editor with no language support for
// form/text/multipart bodies) — CodeMirror 6 under the hood, themed off
// this app's own `--color-*` custom properties so it tracks the OS light/
// dark scheme like the rest of the UI. Property names, strings, numbers,
// booleans, and tag/attribute names are colored distinctly per CodeMirror's
// own token classification for the language in use; a malformed JSON body
// gets a red underline plus a gutter marker at the error location.
//
// This component only formats and highlights what's already in `modelValue`
// — it never invents or rewrites body content itself. Nothing here decides
// *how* a body serializes to/from a `.nova` file; that stays entirely in
// nova-engine (see `RequestBody::from_text`/`to_body_text`).
import { onBeforeUnmount, onMounted, ref, shallowRef, watch } from "vue";
import { EditorState, type Extension } from "@codemirror/state";
import { EditorView, keymap, lineNumbers, highlightActiveLine } from "@codemirror/view";
import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
import {
  bracketMatching,
  HighlightStyle,
  indentOnInput,
  syntaxHighlighting,
} from "@codemirror/language";
import { tags } from "@lezer/highlight";
import { json, jsonParseLinter } from "@codemirror/lang-json";
import { xml } from "@codemirror/lang-xml";
import { javascript } from "@codemirror/lang-javascript";
import { html } from "@codemirror/lang-html";
import { lintGutter, linter } from "@codemirror/lint";

export type EditorLanguage = "json" | "xml" | "javascript" | "html" | "text";

const props = defineProps<{
  modelValue: string;
  language: EditorLanguage;
  readonly?: boolean;
}>();

const emit = defineEmits<{
  (e: "update:modelValue", value: string): void;
}>();

const host = ref<HTMLDivElement | null>(null);
const view = shallowRef<EditorView | null>(null);

// Token colors as CSS custom properties (defined in _base.scss) so this
// editor stays in sync with the app's light/dark palette without this
// component needing to know which mode is active.
const novaHighlightStyle = HighlightStyle.define([
  { tag: tags.propertyName, color: "var(--color-code-key)", fontWeight: "600" },
  { tag: tags.string, color: "var(--color-code-string)" },
  { tag: [tags.number, tags.bool, tags.null], color: "var(--color-code-number)" },
  { tag: tags.keyword, color: "var(--color-code-keyword)" },
  { tag: tags.tagName, color: "var(--color-code-key)", fontWeight: "600" },
  { tag: tags.attributeName, color: "var(--color-code-keyword)" },
  { tag: tags.attributeValue, color: "var(--color-code-string)" },
  { tag: tags.comment, color: "var(--color-text-muted)", fontStyle: "italic" },
  { tag: tags.invalid, color: "var(--color-danger)", textDecoration: "underline wavy" },
]);

const novaEditorTheme = EditorView.theme({
  "&": {
    color: "var(--color-text)",
    backgroundColor: "var(--color-bg)",
    borderRadius: "var(--code-editor-radius)",
    border: "1px solid var(--color-border)",
    fontSize: "var(--code-editor-font-size)",
  },
  "&.cm-focused": {
    outline: "1px solid var(--color-accent)",
  },
  ".cm-content": {
    fontFamily: "var(--code-editor-font-family)",
    minHeight: "160px",
    padding: "8px 0",
  },
  ".cm-scroller": {
    overflow: "auto",
    maxHeight: "480px",
  },
  ".cm-gutters": {
    backgroundColor: "var(--color-bg)",
    color: "var(--color-text-muted)",
    border: "none",
  },
  ".cm-activeLine": {
    backgroundColor: "var(--color-bg-raised)",
  },
  ".cm-activeLineGutter": {
    backgroundColor: "var(--color-bg-raised)",
  },
  // Lint hover tooltips (e.g. "invalid JSON") render outside `.cm-editor`'s
  // own DOM subtree, so without explicit colors here they fall back to
  // CodeMirror's built-in light-theme tooltip styling — white text on a
  // white background under this app's dark palette. CodeMirror special-
  // cases these selectors when they appear in a theme extension, applying
  // them globally even though the elements render elsewhere.
  ".cm-tooltip": {
    backgroundColor: "var(--color-bg-raised)",
    border: "1px solid var(--color-border)",
    borderRadius: "var(--code-editor-radius)",
    color: "var(--color-text)",
  },
  ".cm-tooltip.cm-tooltip-lint": {
    padding: "0",
  },
  ".cm-diagnostic": {
    padding: "4px 8px",
    borderLeft: "3px solid var(--color-danger)",
    color: "var(--color-text)",
  },
  ".cm-diagnosticText": {
    color: "var(--color-text)",
  },
  ".cm-diagnostic-error": {
    borderLeftColor: "var(--color-danger)",
  },
});

// An empty body is valid (no body sent) — don't flag it as invalid JSON.
function jsonLinterIgnoringEmpty(): ReturnType<typeof jsonParseLinter> {
  const base = jsonParseLinter();
  return (view) => (view.state.doc.toString().trim() === "" ? [] : base(view));
}

function languageExtension(language: EditorLanguage): Extension[] {
  switch (language) {
    case "json":
      return [json(), linter(jsonLinterIgnoringEmpty()), lintGutter()];
    case "xml":
      return [xml()];
    case "javascript":
      return [javascript()];
    case "html":
      return [html()];
    case "text":
    default:
      return [];
  }
}

function buildExtensions(language: EditorLanguage, readonly: boolean): Extension[] {
  return [
    lineNumbers(),
    history(),
    indentOnInput(),
    bracketMatching(),
    highlightActiveLine(),
    keymap.of([...defaultKeymap, ...historyKeymap]),
    syntaxHighlighting(novaHighlightStyle),
    novaEditorTheme,
    EditorView.lineWrapping,
    EditorView.editable.of(!readonly),
    ...languageExtension(language),
    EditorView.updateListener.of((update) => {
      if (update.docChanged) {
        emit("update:modelValue", update.state.doc.toString());
      }
    }),
  ];
}

onMounted(() => {
  if (!host.value) return;
  view.value = new EditorView({
    state: EditorState.create({
      doc: props.modelValue,
      extensions: buildExtensions(props.language, props.readonly ?? false),
    }),
    parent: host.value,
  });
});

onBeforeUnmount(() => {
  view.value?.destroy();
});

// Inserts `text` at the current selection (replacing it if non-empty), or
// at the end of the document if the editor doesn't have one — used by the
// GraphQL schema explorer's click-to-insert. Left as the only way a parent
// reaches into this editor's document; everything else still flows through
// `modelValue`.
function insertAtCursor(text: string) {
  const current = view.value;
  if (!current) return;
  const { from, to } = current.state.selection.main;
  current.dispatch({
    changes: { from, to, insert: text },
    selection: { anchor: from + text.length },
  });
  current.focus();
}

defineExpose({ insertAtCursor });

// The editor is the source of truth for its own document while the user
// types; only push an external `modelValue` change in (e.g. switching to
// a different request) when it didn't just come from this editor itself.
watch(
  () => props.modelValue,
  (next) => {
    const current = view.value;
    if (!current) return;
    if (current.state.doc.toString() === next) return;
    current.dispatch({
      changes: { from: 0, to: current.state.doc.length, insert: next },
    });
  },
);

// Switching request/content-type swaps the language mode by rebuilding
// the editor's state in place (keeps the same DOM host).
watch(
  () => props.language,
  (language) => {
    const current = view.value;
    if (!current) return;
    current.setState(
      EditorState.create({
        doc: current.state.doc,
        extensions: buildExtensions(language, props.readonly ?? false),
      }),
    );
  },
);
</script>

<template>
  <div ref="host" class="code-editor"></div>
</template>
