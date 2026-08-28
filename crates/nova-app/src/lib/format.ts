// Small presentational formatters shared across the response viewer.
export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

export function statusClass(status: number): string {
  if (status >= 200 && status < 300) return "response-status--ok";
  if (status >= 400) return "response-status--error";
  return "response-status--other";
}

// Method badges reuse the app's fixed 4-color palette rather than inventing
// per-method colors: GET/safe -> success, POST/primary -> accent,
// PUT+PATCH/modifies -> warning, DELETE -> danger, anything else -> neutral.
const METHOD_LABELS: Record<string, string> = {
  GET: "GET",
  POST: "POST",
  PUT: "PUT",
  PATCH: "PTCH",
  DELETE: "DEL",
  HEAD: "HEAD",
  OPTIONS: "OPT",
};

export function methodLabel(method: string): string {
  const upper = method.toUpperCase();
  return METHOD_LABELS[upper] ?? upper.slice(0, 4);
}

export function methodClass(method: string): string {
  switch (method.toUpperCase()) {
    case "GET":
      return "method-badge--get";
    case "POST":
      return "method-badge--post";
    case "PUT":
    case "PATCH":
      return "method-badge--modify";
    case "DELETE":
      return "method-badge--delete";
    default:
      return "method-badge--neutral";
  }
}

/** Formats a `HistoryEntry.sent_at_ms`-style epoch-millis timestamp for display in a history list. */
export function formatTimestamp(epochMs: number): string {
  return new Date(epochMs).toLocaleTimeString(undefined, {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}
