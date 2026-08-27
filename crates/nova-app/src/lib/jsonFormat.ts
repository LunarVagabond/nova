// Pretty-prints `text` as JSON for the Body tab's Beautify button, tolerating
// a trailing comma before a closing `}`/`]` — invalid per the JSON spec (and
// still flagged by the editor's own lint gutter, since that reflects what
// the engine's strict parser will actually accept on Send), but common
// enough to type by hand that refusing to format over it would be
// unhelpful. Throws if the text still isn't parseable after that fix-up;
// callers decide what to do with genuinely invalid JSON.
export function beautifyJson(text: string): string {
  const withoutTrailingCommas = text.replace(/,(\s*[}\]])/g, "$1");
  return JSON.stringify(JSON.parse(withoutTrailingCommas), null, 2);
}
