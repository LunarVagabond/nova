// Thin wrapper around the Tauri commands exposed by `nova-app/src-tauri`,
// which are themselves thin wrappers over `nova-engine`. No project/
// environment/collection logic should be reimplemented here — this file
// only calls `invoke` and types the result.

import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";

import type { NovaProject, RequestResponse } from "../types/nova";

export function openProject(path: string): Promise<NovaProject> {
  return invoke<NovaProject>("open_project", { path });
}

export function validateProject(path: string): Promise<string[]> {
  return invoke<string[]>("validate_project", { path });
}

/** Parses, resolves, and executes the `.http` file at `requestPath`. */
export function sendRequest(
  requestPath: string,
  environment: string | null,
): Promise<RequestResponse> {
  return invoke<RequestResponse>("send_request", { requestPath, environment });
}

/** Opens the native folder picker and returns the chosen path, or null if cancelled. */
export async function pickProjectDirectory(): Promise<string | null> {
  const selected = await open({ directory: true, multiple: false });
  return typeof selected === "string" ? selected : null;
}
