// Finding and describing `{{variable}}` placeholders in plain text, shared
// by `VariableAwareInput.vue` (single-line fields) and `CodeEditor.vue`'s
// variable-highlight extension (multi-line body/script editors) so both
// surfaces treat a placeholder the same way. Mirrors
// `nova-engine::request::resolve::substitute`'s own parsing: a name is
// whatever sits between `{{` and the next `}}` (trimmed), with no
// restriction on characters, and a `{{` with no closing `}}` isn't a
// placeholder at all.
import type { ResolvedVariables } from "../types/nova";

const PLACEHOLDER_PATTERN = /\{\{([^{}]*)\}\}/g;

export interface VariableToken {
  /** Offset of the placeholder's opening `{{` within the source text. */
  start: number;
  /** Offset just past the placeholder's closing `}}`. */
  end: number;
  /** The trimmed name between the braces, e.g. `base_url`. */
  name: string;
}

/** Every `{{name}}` placeholder in `text`, in source order. */
export function findVariableTokens(text: string): VariableToken[] {
  const tokens: VariableToken[] = [];
  for (const match of text.matchAll(PLACEHOLDER_PATTERN)) {
    tokens.push({ start: match.index, end: match.index + match[0].length, name: match[1].trim() });
  }
  return tokens;
}

export interface VariableTooltip {
  /** What to show in the hover tooltip for this placeholder's name — just
   * the resolved value itself (or a short status message when there isn't
   * one), not a "name = value" pair — the placeholder text right there in
   * the field already says the name. */
  text: string;
  /** Whether the name resolved to a value the environment flags secret. */
  isSecret: boolean;
  /** Whether the name resolved to nothing (undefined variable). */
  isUndefined: boolean;
}

/**
 * Describe what `name` resolves to, for a hover tooltip — against
 * `resolved` (the request panel's already-loaded `ResolvedVariables`, or
 * `null` while it's still loading/unavailable). The value itself is always
 * shown in full, secret-flagged or not — this is a local, read-only
 * quick-glance over your own already-open environment file, the same trust
 * boundary as opening that file directly, so masking it here would just add
 * friction without hiding anything a click into the environment editor
 * wouldn't already reveal.
 */
export function describeVariable(
  name: string,
  resolved: ResolvedVariables | null | undefined,
): VariableTooltip {
  if (name.startsWith("$")) {
    return { text: "computed fresh when sent", isSecret: false, isUndefined: false };
  }
  if (!resolved) {
    return { text: "resolving…", isSecret: false, isUndefined: false };
  }
  if (!(name in resolved.variables)) {
    return { text: "undefined in this environment", isSecret: false, isUndefined: true };
  }
  return { text: resolved.variables[name], isSecret: resolved.secrets.includes(name), isUndefined: false };
}
