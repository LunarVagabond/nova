// Thin wrapper around the Tauri commands exposed by `nova-app/src-tauri`,
// which are themselves thin wrappers over `nova-engine`. No project/
// environment/collection logic should be reimplemented here — this file
// only calls `invoke` and types the result.

import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";

import type {
  Manifest,
  NovaProject,
  ParsedCurlRequest,
  QueryParam,
  RequestDraft,
  RequestFile,
  RequestHeader,
  RequestResponse,
} from "../types/nova";

export function openProject(path: string): Promise<NovaProject> {
  return invoke<NovaProject>("open_project", { path });
}

export function validateProject(path: string): Promise<string[]> {
  return invoke<string[]>("validate_project", { path });
}

/** Parses, resolves, and executes the `.nova` file at `requestPath`. */
export function sendRequest(
  requestPath: string,
  environment: string | null,
): Promise<RequestResponse> {
  return invoke<RequestResponse>("send_request", { requestPath, environment });
}

/** Parses the `.nova` file at `requestPath` into an editable draft. */
export function readRequest(requestPath: string): Promise<RequestDraft> {
  return invoke<RequestDraft>("read_request", { requestPath });
}

/**
 * Writes edited method/URL/query/headers/body back to the `.nova` file at
 * `requestPath`. Any assertions, extractions, and example response
 * already in the file are preserved unchanged.
 */
export function saveRequest(
  requestPath: string,
  draft: {
    method: string;
    url: string;
    query: QueryParam[];
    headers: RequestHeader[];
    body: string;
  },
): Promise<void> {
  return invoke<void>("save_request", {
    requestPath,
    method: draft.method,
    url: draft.url,
    query: draft.query,
    headers: draft.headers,
    body: draft.body,
  });
}

/**
 * Creates a new `.nova` file named `name` (a `.nova` suffix is added if
 * missing) directly inside the collection directory at `collectionPath`,
 * with a minimal default request, and returns its `RequestFile` handle.
 */
export function createRequest(collectionPath: string, name: string): Promise<RequestFile> {
  return invoke<RequestFile>("create_request", { collectionPath, name });
}

/** Parses a pasted `curl`/`wget` command into method/URL/headers/body. */
export function parseCurlCommand(command: string): Promise<ParsedCurlRequest> {
  return invoke<ParsedCurlRequest>("parse_curl_command", { command });
}

/**
 * Writes an edited manifest back to `projectRoot`'s `nova.yaml`, replacing
 * it entirely. `projectRoot` is the project's Nova directory
 * (`NovaProject.root`, e.g. `<repo>/nova`), not the outer repo root.
 */
export function saveManifest(projectRoot: string, manifest: Manifest): Promise<void> {
  return invoke<void>("save_manifest", { projectRoot, manifest });
}

/** Opens the native folder picker and returns the chosen path, or null if cancelled. */
export async function pickProjectDirectory(): Promise<string | null> {
  const selected = await open({ directory: true, multiple: false });
  return typeof selected === "string" ? selected : null;
}
