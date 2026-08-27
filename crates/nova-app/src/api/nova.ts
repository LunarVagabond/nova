// Thin wrapper around the Tauri commands exposed by `nova-app/src-tauri`,
// which are themselves thin wrappers over `nova-engine`. No project/
// environment/collection logic should be reimplemented here — this file
// only calls `invoke` and types the result.

import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";

import type {
  AuthScheme,
  Collection,
  GitStatusMap,
  InitOutcome,
  Manifest,
  NovaEnvironment,
  OpenProjectOutcome,
  ParsedCurlRequest,
  RequestDraft,
  RequestFile,
  RequestResponse,
} from "../types/nova";

/**
 * Opens the project at `path`. A directory with no project in it resolves
 * to `"not_found"` rather than rejecting, so the caller can offer to
 * create one; a project that exists but is broken still rejects.
 */
export function openProject(path: string): Promise<OpenProjectOutcome> {
  return invoke<OpenProjectOutcome>("open_project", { path });
}

/**
 * Scaffolds a brand-new Nova project under `path/nova/`, the same way
 * `nova init` does. A null or blank `name` defaults to the target
 * directory's name; `installHook` adds the opt-in `check-secrets` git
 * pre-commit hook.
 */
export function initProject(
  path: string,
  options: { name: string | null; installHook: boolean },
): Promise<InitOutcome> {
  return invoke<InitOutcome>("init_project", {
    path,
    name: options.name,
    installHook: options.installHook,
  });
}

export function validateProject(path: string): Promise<string[]> {
  return invoke<string[]>("validate_project", { path });
}

/**
 * Per-file git status for the project at `path`, keyed by absolute path —
 * `null` when `path` isn't inside a git repository at all.
 */
export function gitStatus(path: string): Promise<GitStatusMap | null> {
  return invoke<GitStatusMap | null>("git_status", { path });
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
 * Writes an edited draft — method/URL/query/headers/body, plus the
 * request's auth scheme and settings — back to the `.nova` file at
 * `requestPath`. Any assertions, extractions, and example response
 * already in the file are preserved unchanged.
 */
export function saveRequest(requestPath: string, draft: RequestDraft): Promise<void> {
  return invoke<void>("save_request", { requestPath, draft });
}

/**
 * Creates a new `.nova` file named `name` (a `.nova` suffix is added if
 * missing) directly inside the collection directory at `collectionPath`,
 * with a minimal default request, and returns its `RequestFile` handle.
 */
export function createRequest(collectionPath: string, name: string): Promise<RequestFile> {
  return invoke<RequestFile>("create_request", { collectionPath, name });
}

/** Deletes the request file at `requestPath`. */
export function deleteRequest(requestPath: string): Promise<void> {
  return invoke<void>("delete_request", { requestPath });
}

/**
 * Renames the request file at `requestPath` to `newName` (a `.nova` suffix
 * is added if missing), keeping it in the same collection directory.
 */
export function renameRequest(requestPath: string, newName: string): Promise<RequestFile> {
  return invoke<RequestFile>("rename_request", { requestPath, newName });
}

/**
 * Duplicates the request file at `requestPath` to `newName` inside the same
 * collection directory, copying its contents.
 */
export function duplicateRequest(requestPath: string, newName: string): Promise<RequestFile> {
  return invoke<RequestFile>("duplicate_request", { requestPath, newName });
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

/**
 * Creates a new, empty subcollection named `name` directly inside the
 * collection directory at `parentPath`, and returns its `Collection` handle.
 */
export function createCollection(parentPath: string, name: string): Promise<Collection> {
  return invoke<Collection>("create_collection", { parentPath, name });
}

/**
 * Renames the collection directory at `collectionPath` to `newName`,
 * keeping it in the same parent directory; all of its contents (nested
 * subcollections and requests) move with it.
 */
export function renameCollection(collectionPath: string, newName: string): Promise<Collection> {
  return invoke<Collection>("rename_collection", { collectionPath, newName });
}

/** Deletes the collection directory at `collectionPath` and everything inside it. */
export function deleteCollection(collectionPath: string): Promise<void> {
  return invoke<void>("delete_collection", { collectionPath });
}

/**
 * Creates a new environment file named `name` directly inside the
 * environments directory at `environmentsDir` (a project's
 * `NovaProject.environments_dir`), with no variables set, and returns its
 * `NovaEnvironment` handle.
 */
export function createEnvironment(environmentsDir: string, name: string): Promise<NovaEnvironment> {
  return invoke<NovaEnvironment>("create_environment", { environmentsDir, name });
}

/**
 * Writes an edited environment's name/variables/default auth scheme back
 * to the file at `environmentPath`, replacing whatever was there. If the
 * name changed and this was the project's default environment,
 * `projectRoot`'s manifest is updated to follow the rename.
 */
export function saveEnvironment(
  projectRoot: string,
  environmentPath: string,
  previousName: string,
  environment: { name: string; variables: Record<string, string>; auth: AuthScheme | null },
): Promise<void> {
  return invoke<void>("save_environment", {
    projectRoot,
    environmentPath,
    previousName,
    name: environment.name,
    variables: environment.variables,
    auth: environment.auth,
  });
}

/** Deletes the environment file at `environmentPath`. */
export function deleteEnvironment(environmentPath: string): Promise<void> {
  return invoke<void>("delete_environment", { environmentPath });
}

/** Opens the native folder picker and returns the chosen path, or null if cancelled. */
export async function pickProjectDirectory(): Promise<string | null> {
  const selected = await open({ directory: true, multiple: false });
  return typeof selected === "string" ? selected : null;
}
