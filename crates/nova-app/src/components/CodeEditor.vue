<script setup lang="ts">
// A syntax-highlighted, lint-aware code editor — used for the request body
// field (JSON/XML today; a plain editor with no language support for
// form/text/multipart bodies) and for the Scripts tab's in-app pre-/post-
// request script editor (JavaScript/Python; anything else, e.g. a custom
// interpreter mapping, falls back to plain text) — CodeMirror 6 under the
// hood, themed off this app's own `--color-*` custom properties so it
// tracks the OS light/dark scheme like the rest of the UI. Property names,
// strings, numbers, booleans, and tag/attribute names are colored
// distinctly per CodeMirror's own token classification for the language in
// use; a malformed JSON body, or a JS/Python script with a syntax error,
// gets a red underline plus a gutter marker at the error location — see
// `syntaxErrorLinter` for what "lint" does and doesn't mean for JS/Python
// here.
//
// This component only formats and highlights what's already in `modelValue`
// — it never invents or rewrites body content itself. Nothing here decides
// *how* a body serializes to/from a `.nova` file; that stays entirely in
// nova-engine (see `RequestBody::from_text`/`to_body_text`).
import { onBeforeUnmount, onMounted, ref, shallowRef, watch } from "vue";
import { EditorState, RangeSetBuilder, type Extension } from "@codemirror/state";
import {
  Decoration,
  type DecorationSet,
  EditorView,
  hoverTooltip,
  keymap,
  lineNumbers,
  highlightActiveLine,
  ViewPlugin,
  type ViewUpdate,
} from "@codemirror/view";
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
import { javascript, javascriptLanguage } from "@codemirror/lang-javascript";
import { python, pythonLanguage } from "@codemirror/lang-python";
import { html } from "@codemirror/lang-html";
import { lintGutter, linter, type Diagnostic } from "@codemirror/lint";
import type { LRLanguage } from "@codemirror/language";
import { describeVariable, findVariableTokens } from "../lib/variableTokens";
import type { ResolvedVariables } from "../types/nova";

export type EditorLanguage = "json" | "xml" | "javascript" | "python" | "html" | "text";

const props = defineProps<{
  modelValue: string;
  language: EditorLanguage;
  readonly?: boolean;
  /**
   * This request's resolved `{{variable}}` values, for hover tooltips over
   * a placeholder in the document — optional, since not every caller has
   * one loaded (or a placeholder to highlight in the first place).
   */
  resolvedVariables?: ResolvedVariables | null;
}>();

// A live handle the hover-tooltip callback below reads at hover time —
// CodeMirror extensions are built once per editor instance (`onMounted`),
// but `resolvedVariables` can keep changing after that (a different
// environment picked, a variable edited), so this ref, not the prop value
// itself, is what the extension closes over.
const liveResolvedVariables = shallowRef<ResolvedVariables | null>(props.resolvedVariables ?? null);
watch(
  () => props.resolvedVariables,
  (next) => {
    liveResolvedVariables.value = next ?? null;
  },
);

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
  // A `{{variable}}` placeholder, colored to stand out from the
  // surrounding string/text it sits in — see `variableHighlightPlugin`
  // below for what marks the range.
  ".cm-nova-variable": {
    color: "var(--color-accent)",
    fontWeight: "600",
  },
  ".cm-nova-variable-tooltip": {
    padding: "4px 8px",
    maxWidth: "24rem",
    wordBreak: "break-word",
  },
});

// Marks every `{{variable}}` placeholder in the document with a
// `.cm-nova-variable` decoration, recomputed whenever the document changes.
function buildVariableDecorations(view: EditorView): DecorationSet {
  const builder = new RangeSetBuilder<Decoration>();
  for (const token of findVariableTokens(view.state.doc.toString())) {
    builder.add(token.start, token.end, Decoration.mark({ class: "cm-nova-variable" }));
  }
  return builder.finish();
}

const variableHighlightPlugin = ViewPlugin.fromClass(
  class {
    decorations: DecorationSet;
    constructor(view: EditorView) {
      this.decorations = buildVariableDecorations(view);
    }
    update(update: ViewUpdate) {
      if (update.docChanged) this.decorations = buildVariableDecorations(update.view);
    }
  },
  { decorations: (instance) => instance.decorations },
);

// Hovering a `{{variable}}` placeholder shows what it resolves to —
// `getResolved` is read at hover time (see `liveResolvedVariables` above),
// not captured once when the extension is built.
function variableHoverTooltip(getResolved: () => ResolvedVariables | null) {
  return hoverTooltip((view, pos) => {
    const token = findVariableTokens(view.state.doc.toString()).find(
      (candidate) => pos >= candidate.start && pos <= candidate.end,
    );
    if (!token) return null;
    const { text } = describeVariable(token.name, getResolved());
    return {
      pos: token.start,
      end: token.end,
      above: true,
      create() {
        const dom = document.createElement("div");
        dom.className = "cm-nova-variable-tooltip";
        dom.textContent = text;
        return { dom };
      },
    };
  });
}

// An empty body is valid (no body sent) — don't flag it as invalid JSON.
function jsonLinterIgnoringEmpty(): ReturnType<typeof jsonParseLinter> {
  const base = jsonParseLinter();
  return (view) => (view.state.doc.toString().trim() === "" ? [] : base(view));
}

// "Lint" for JavaScript/Python scripts (the Scripts tab's #184 in-app
// editor) is scoped to syntax-error detection, not a real linter — there's
// no ESLint/pylint dependency here, just each language's own CodeMirror
// grammar. Reusing the grammar this way (rather than executing the script,
// e.g. via `new Function(...)`) never runs anything the user typed; it
// just walks the parse tree `language.parser.parse()` already produces for
// syntax highlighting and flags any node the parser couldn't make sense of
// as an error node (`type.isError`). An empty document is never flagged —
// nothing to have a syntax error in.
function syntaxErrorLinter(language: LRLanguage): (view: EditorView) => Diagnostic[] {
  return (view) => {
    const text = view.state.doc.toString();
    if (text.trim() === "") return [];
    const tree = language.parser.parse(text);
    const diagnostics: Diagnostic[] = [];
    tree.iterate({
      enter: (node) => {
        if (node.type.isError) {
          diagnostics.push({
            from: node.from,
            to: Math.max(node.to, node.from + 1),
            severity: "error",
            message: "syntax error",
          });
        }
      },
    });
    return diagnostics;
  };
}

function languageExtension(language: EditorLanguage): Extension[] {
  switch (language) {
    case "json":
      return [json(), linter(jsonLinterIgnoringEmpty()), lintGutter()];
    case "xml":
      return [xml()];
    case "javascript":
      return [javascript(), linter(syntaxErrorLinter(javascriptLanguage)), lintGutter()];
    case "python":
      return [python(), linter(syntaxErrorLinter(pythonLanguage)), lintGutter()];
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
    variableHighlightPlugin,
    variableHoverTooltip(() => liveResolvedVariables.value),
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
