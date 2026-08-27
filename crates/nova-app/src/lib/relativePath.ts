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
 * Returns `filePath` relative to `root`, joined with `/` — or `null` if
 * `filePath` doesn't actually live under `root` at all.
 *
 * A `MultipartField.file_path` is only ever meant to be a project-relative
 * reference (the engine rejects anything else at send time — see
 * `nova_engine::execute::resolve_multipart_file_path`), so a picked file
 * outside the project has nothing sensible to write here; returning `null`
 * lets the caller refuse the attachment instead of silently saving a path
 * that won't resolve for anyone else who opens the project, or resolves to
 * something else entirely.
 */
export function relativeToRoot(root: string, filePath: string): string | null {
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
    return null;
  }

  return fileSegments.slice(i).join("/");
}
