// Best-effort re-indenter for the Scripts tab editor's Beautify button on a
// JavaScript pre-/post-request script — mirrors `formatXml`'s scope: this
// re-derives each line's indentation from `{}`/`[]`/`()` bracket depth, it
// doesn't reflow or reformat the tokens within a line the way a real
// formatter (Prettier et al.) would. It also doesn't understand strings,
// template literals, regex literals, or comments, so a brace/bracket
// character inside any of those throws off the depth count for the rest of
// the file — acceptable for the common case of already-mostly-indented
// scripts run through this to tidy up, not a substitute for a real JS
// formatter (out of scope here as a disproportionately large dependency
// for this button). Python has no beautify at all for the same reason; see
// `CodeEditor.vue`.
export function beautifyJavascript(text: string): string {
  const lines = text.split("\n");
  let indent = 0;
  const result: string[] = [];
  for (const rawLine of lines) {
    const line = rawLine.trim();
    if (line === "") {
      result.push("");
      continue;
    }
    const leadingClosers = line.match(/^[)}\]]+/);
    if (leadingClosers) {
      indent = Math.max(indent - leadingClosers[0].length, 0);
    }
    result.push("  ".repeat(indent) + line);
    const opens = (line.match(/[{[(]/g) ?? []).length;
    const closes = (line.match(/[}\])]/g) ?? []).length;
    indent = Math.max(indent + opens - closes, 0);
  }
  return result.join("\n");
}
