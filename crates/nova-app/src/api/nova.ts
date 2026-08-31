// Thin wrapper around the Tauri commands exposed by `nova-app/src-tauri`,
// which are themselves thin wrappers over `nova-engine`. No project/
// environment/collection logic should be reimplemented here — this file
// only calls `invoke` and types the result.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open, save } from "@tauri-apps/plugin-dialog";

import type {
  AuthScheme,
  Collection,
  CookieView,
  GitStatusMap,
  GraphQlBody,
  HistoryDetail,
  HistorySummary,
  ImportProjectOutcome,
  InitOutcome,
  Manifest,
  MockServerStatus,
  MultipartField,
  NovaEnvironment,
  OpenProjectOutcome,
  ParsedCurlRequest,
  RequestDraft,
  RequestFile,
  RequestHeader,
  RequestResponse,
  ResponseDiff,
  TestRunResult,
  WebSocketDraft,
  WebSocketExchange,
  WebSocketSessionStatus,
  WsSessionMessageEvent,
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

/**
 * The full variable map `requestPath`'s `{{name}}` placeholders would
 * resolve against right now — against `environment` if named (else the
 * project's default), collection variables and this project's
 * session-chained variables included — without sending anything. Powers
 * the request panel's read-only variables drawer.
 */
export function getResolvedVariables(
  requestPath: string,
  environment: string | null,
): Promise<Record<string, string>> {
  return invoke<Record<string, string>>("get_resolved_variables", { requestPath, environment });
}

/**
 * The project at `path`'s recent request/response history, most-recent
 * first — empty (not an error) if nothing has been sent in this project
 * yet this session. History lives only for the life of the app session;
 * closing and reopening the project starts it over.
 */
export function getHistory(path: string): Promise<HistorySummary[]> {
  return invoke<HistorySummary[]>("get_history", { path });
}

/** Reopens one past history entry from the project at `path` by the `id` `getHistory` handed out for it. */
export function reopenHistoryEntry(path: string, id: number): Promise<HistoryDetail> {
  return invoke<HistoryDetail>("reopen_history_entry", { path, id });
}

/**
 * The project at `path`'s currently-stored session cookies — empty (not an
 * error) if nothing has set a cookie in this project yet this session.
 * Cookies live only for the life of the app session, the same as history.
 */
export function getCookies(path: string): Promise<CookieView[]> {
  return invoke<CookieView[]>("get_cookies", { path });
}

/** Deletes one stored cookie (identified by `host` + `name`) from the project at `path`'s session. */
export function deleteCookie(path: string, host: string, name: string): Promise<boolean> {
  return invoke<boolean>("delete_cookie", { path, host, name });
}

/** Deletes every stored cookie from the project at `path`'s session. */
export function clearCookies(path: string): Promise<void> {
  return invoke<void>("clear_cookies", { path });
}

/**
 * Edits the value of one stored cookie (identified by `host` + `name`) in
 * the project at `path`'s session, leaving its other attributes untouched.
 */
export function updateCookie(
  path: string,
  host: string,
  name: string,
  value: string,
): Promise<boolean> {
  return invoke<boolean>("update_cookie", { path, host, name, value });
}

/**
 * Diffs the most recent send of the `.nova` file at `requestPath` against
 * the send immediately before it in this project's session history — "did
 * this response change since the last time it ran". Resolves to `null`
 * (not an error) when there isn't a pair of matching history entries yet
 * to compare.
 */
export function diffAgainstPreviousRun(
  requestPath: string,
  environment: string | null,
): Promise<ResponseDiff | null> {
  return invoke<ResponseDiff | null>("diff_against_previous_run", { requestPath, environment });
}

/**
 * Diffs the most recent send of the `.nova` file at `requestPath` against
 * its own hand-written `[response]` example, if it has one — "did this
 * response drift from the documented example". Resolves to `null` (not an
 * error) when the request has no example, or hasn't been sent yet this
 * session.
 */
export function diffAgainstExampleResponse(
  requestPath: string,
  environment: string | null,
): Promise<ResponseDiff | null> {
  return invoke<ResponseDiff | null>("diff_against_example_response", { requestPath, environment });
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

/** Parses the `.nova` file at `requestPath` as a WebSocket connection declaration into an editable draft. */
export function readWebSocketRequest(requestPath: string): Promise<WebSocketDraft> {
  return invoke<WebSocketDraft>("read_websocket_request", { requestPath });
}

/** Writes an edited WebSocket draft — URL/headers/messages — back to the `.nova` file at `requestPath`. */
export function saveWebSocketRequest(requestPath: string, draft: WebSocketDraft): Promise<void> {
  return invoke<void>("save_websocket_request", { requestPath, draft });
}

/**
 * Parses, resolves, connects to, and exchanges messages with the WebSocket
 * connection the `.nova` file at `requestPath` declares, against
 * `environment` if named (else the project's default). Resolves once the
 * connection closes or the read timeout elapses with nothing further
 * coming in.
 */
export function connectWebSocket(
  requestPath: string,
  environment: string | null,
): Promise<WebSocketExchange> {
  return invoke<WebSocketExchange>("connect_websocket", { requestPath, environment });
}

/**
 * Creates a new `.nova` file named `name` (a `.nova` suffix is added if
 * missing) directly inside the collection directory at `collectionPath`,
 * declaring a WebSocket connection rather than an HTTP request, and
 * returns its `RequestFile` handle.
 */
export function createWebSocketRequest(collectionPath: string, name: string): Promise<RequestFile> {
  return invoke<RequestFile>("create_websocket_request", { collectionPath, name });
}

/**
 * Opens an interactive WebSocket session against the `.nova` file at
 * `requestPath` (resolving `{{variable}}`s against `environment` if named,
 * else the project's default) and keeps it open — the GUI-only counterpart
 * to `connectWebSocket`'s one-shot batch flow. Each received message
 * arrives as a `"ws-session:message"` event (see `listenForWebSocketSessionMessages`
 * below); an unexpected close arrives as `"ws-session:closed"`. Rejects if a
 * session is already open.
 */
export function connectWebSocketSession(requestPath: string, environment: string | null): Promise<void> {
  return invoke<void>("connect_websocket_session", { requestPath, environment });
}

/** Sends `text` on the currently-open interactive WebSocket session. */
export function sendWebSocketSessionMessage(text: string): Promise<void> {
  return invoke<void>("send_websocket_session_message", { text });
}

/** Closes the currently-open interactive WebSocket session, if any. */
export function disconnectWebSocketSession(): Promise<void> {
  return invoke<void>("disconnect_websocket_session");
}

/** Whether an interactive WebSocket session is currently open. */
export function websocketSessionStatus(): Promise<WebSocketSessionStatus> {
  return invoke<WebSocketSessionStatus>("websocket_session_status");
}

/**
 * Subscribes to every text message the currently-open interactive
 * WebSocket session receives, in arrival order, until the returned
 * unlisten function is called. Thin wrapper around `@tauri-apps/api/event`'s
 * `listen` so components don't import it (and the event name) directly.
 */
export function listenForWebSocketSessionMessages(
  handler: (message: WsSessionMessageEvent) => void,
): Promise<UnlistenFn> {
  return listen<WsSessionMessageEvent>("ws-session:message", (event) => handler(event.payload));
}

/**
 * Subscribes to the currently-open interactive WebSocket session ending on
 * its own (the server closed it, or a read failed) — not fired for an
 * explicit `disconnectWebSocketSession()` call, since the caller already
 * knows about that one.
 */
export function listenForWebSocketSessionClosed(handler: () => void): Promise<UnlistenFn> {
  return listen("ws-session:closed", () => handler());
}

/**
 * Parses a multipart body's raw wire text (the same text `RequestDraft.body_text`
 * carries) into structured fields, for the Body tab's multipart field table.
 */
export function parseMultipartBody(
  headers: RequestHeader[],
  bodyText: string,
): Promise<MultipartField[]> {
  return invoke<MultipartField[]>("parse_multipart_body", { headers, bodyText });
}

/**
 * Serializes structured multipart fields back to the raw wire text a
 * `.nova` file's `[body]` marker would hold for them — the inverse of
 * `parseMultipartBody`.
 */
export function serializeMultipartBody(
  fields: MultipartField[],
  headers: RequestHeader[],
): Promise<string> {
  return invoke<string>("serialize_multipart_body", { fields, headers });
}

/**
 * Parses a GraphQL body's raw wire text (the same text `RequestDraft.body_text`
 * carries) into its query/variables/operation name, for the Body tab's
 * GraphQL query+variables editor.
 */
export function parseGraphqlBody(bodyText: string): Promise<GraphQlBody> {
  return invoke<GraphQlBody>("parse_graphql_body_text", { bodyText });
}

/**
 * Serializes a GraphQL query/variables/operation name back to the raw wire
 * text a `.nova` file's `[body]` marker would hold for it — the inverse of
 * `parseGraphqlBody`.
 */
export function serializeGraphqlBody(graphql: GraphQlBody): Promise<string> {
  return invoke<string>("serialize_graphql_body", { graphql });
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

/**
 * Runs every request under `path` (a whole project, a collection
 * subdirectory, or a single request) as a test — parsing, resolving,
 * executing, and evaluating its assertions, the same way `nova test` does
 * — against `environment` if named (else the project's default).
 */
export function runTests(path: string, environment: string | null): Promise<TestRunResult> {
  return invoke<TestRunResult>("run_tests", { path, environment });
}

/** Current state of the desktop app's mock server toggle. */
export function mockServerStatus(): Promise<MockServerStatus> {
  return invoke<MockServerStatus>("mock_server_status");
}

/**
 * Starts the mock server for the project at `path`, serving each
 * discovered request's example response the same way `nova mock` does.
 * `host`/`port` default to the CLI's own defaults (`127.0.0.1:4010`) when
 * omitted.
 */
export function startMockServer(
  path: string,
  options?: { host?: string; port?: number },
): Promise<MockServerStatus> {
  return invoke<MockServerStatus>("start_mock_server", {
    path,
    host: options?.host ?? null,
    port: options?.port ?? null,
  });
}

/** Stops the desktop app's mock server. A no-op if it isn't running. */
export function stopMockServer(): Promise<MockServerStatus> {
  return invoke<MockServerStatus>("stop_mock_server");
}

/** Opens the native folder picker and returns the chosen path, or null if cancelled. */
export async function pickProjectDirectory(): Promise<string | null> {
  const selected = await open({ directory: true, multiple: false });
  return typeof selected === "string" ? selected : null;
}

/** Opens the native file picker and returns the chosen file's path, or null if cancelled. */
export async function pickFile(): Promise<string | null> {
  const selected = await open({ directory: false, multiple: false });
  return typeof selected === "string" ? selected : null;
}

/**
 * Opens the native file picker, filtered to OpenAPI/Postman collection
 * files, and returns the chosen file's path (or null if cancelled) — for
 * the import dialog's "pick a spec/collection to import" step.
 */
export async function pickImportSource(): Promise<string | null> {
  const selected = await open({
    directory: false,
    multiple: false,
    filters: [{ name: "OpenAPI spec / Postman collection", extensions: ["yaml", "yml", "json"] }],
  });
  return typeof selected === "string" ? selected : null;
}

/**
 * Opens the native folder picker for "where should the imported project be
 * written" and returns the chosen path, or null if cancelled. A thin,
 * separately-named wrapper over `pickProjectDirectory` so the import
 * dialog's intent reads clearly at the call site even though the
 * underlying picker is identical.
 */
export function pickImportDestination(): Promise<string | null> {
  return pickProjectDirectory();
}

/**
 * Opens the native save-file picker, defaulting to `openapi.yaml`, and
 * returns the chosen destination path (or null if cancelled) — for the
 * export dialog's "where should the OpenAPI spec be written" step.
 */
export async function pickExportDestination(): Promise<string | null> {
  const selected = await save({
    defaultPath: "openapi.yaml",
    filters: [{ name: "OpenAPI spec (YAML)", extensions: ["yaml", "yml"] }],
  });
  return selected ?? null;
}

/**
 * Generates a new Nova project from an OpenAPI 3.x spec or a Postman
 * Collection Format v2.1 export at `inputPath`, and writes it under
 * `outputPath/nova/` — the same thing `nova generate` does on the CLI.
 */
export function importProject(inputPath: string, outputPath: string): Promise<ImportProjectOutcome> {
  return invoke<ImportProjectOutcome>("import_project", { inputPath, outputPath });
}

/**
 * Exports the project at `projectRoot`'s collections as an OpenAPI 3.x
 * spec (YAML), written to `outputPath` — the same thing `nova export` does
 * on the CLI.
 */
export function exportProject(projectRoot: string, outputPath: string): Promise<void> {
  return invoke<void>("export_project", { projectRoot, outputPath });
}
