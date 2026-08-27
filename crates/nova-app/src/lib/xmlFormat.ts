// Best-effort XML indenter for the Body tab's Beautify button — inserts a
// newline between any `>` immediately followed by `<` (i.e. between sibling
// tags, never inside a leaf element's own text content, since there's
// always non-`<` text between that pair there) and indents based on
// open/close tags. Not a full XML formatter (doesn't track void/self-closing
// exceptions beyond a simple regex), but good enough for typical request
// bodies the same way Postman's own "Beautify" is.
export function formatXml(xml: string): string {
  const trimmed = xml.trim();
  if (!trimmed) return xml;
  const withBreaks = trimmed.replace(/(>)(<)(\/*)/g, "$1\n$2$3");
  const lines = withBreaks
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean);

  let pad = 0;
  const result: string[] = [];
  for (const line of lines) {
    let indentNext = 0;
    if (/^<\/\w/.test(line)) {
      pad = Math.max(pad - 1, 0);
    } else if (/^<\?/.test(line) || /^<!/.test(line)) {
      // Processing instruction / comment / doctype — never indents.
    } else if (/^<\w[^>]*[^/]>$/.test(line)) {
      indentNext = 1;
    }
    result.push("  ".repeat(pad) + line);
    pad += indentNext;
  }
  return result.join("\n");
}
