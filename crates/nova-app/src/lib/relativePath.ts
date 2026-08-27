// Turns an absolute file path into a path relative to a project root, for
// the multipart body editor's file attachments — a `MultipartField.file_path`
// is always stored relative to the project root (see `nova_engine::execute`),
// never as an absolute path baked into the `.nova` file.
//
// Handles both `/` and `\` separators so a path picked via the native file
// dialog on Windows still comes out as a portable, forward-slash-joined
// relative path — the same convention `.nova` files already use everywhere
// else (e.g. `{{base_url}}/path`).

function splitSegments(path: string): string[] {
  return path.split(/[\\/]+/).filter((segment) => segment.length > 0);
}

/**
 * Returns `filePath` relative to `root`, joined with `/`. Falls back to
 * `filePath` unchanged (segments as-is) if it doesn't actually live under
 * `root` — the caller is expected to warn rather than silently write a
 * path that won't resolve for anyone else who opens the project.
 */
export function relativeToRoot(root: string, filePath: string): string {
  const rootSegments = splitSegments(root);
  const fileSegments = splitSegments(filePath);

  let i = 0;
  while (
    i < rootSegments.length &&
    i < fileSegments.length &&
    rootSegments[i].toLowerCase() === fileSegments[i].toLowerCase()
  ) {
    i++;
  }

  if (i !== rootSegments.length) {
    // `filePath` isn't under `root` at all — nothing sensible to make
    // relative to, so hand back the original segments joined portably.
    return fileSegments.join("/");
  }

  return fileSegments.slice(i).join("/");
}
