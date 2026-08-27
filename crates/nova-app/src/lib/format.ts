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
