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
import { lintGutter, linter } from "@codemirror/lint";

export type EditorLanguage = "json" | "xml" | "text";

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
});

function languageExtension(language: EditorLanguage): Extension[] {
  switch (language) {
    case "json":
      return [json(), linter(jsonParseLinter()), lintGutter()];
    case "xml":
      return [xml()];
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
