// Mirrors the `Serialize` shape of the types in `crates/nova-engine/src`.
// These are plain data shapes coming back from Tauri commands — no logic
// belongs here. If a field is added/renamed on the Rust side, update it
// here to match; the engine remains the single source of truth for shape.

export interface ProjectInfo {
  name: string;
}

export interface Defaults {
  environment: string | null;
  timeout: string | null;
}

export interface PathConfig {
  path: string;
}

export interface Manifest {
  version: number;
  project: ProjectInfo;
  defaults: Defaults;
  collections: PathConfig;
  environments: PathConfig;
}

/** Mirrors `nova_engine::ApiKeyLocation`. */
export type ApiKeyLocation = "header" | "query";

/**
 * Mirrors `nova_engine::AuthScheme` — a structured authentication scheme,
 * declared either by a request's own `[auth]` section or as an
 * environment-wide default. Serialized by serde as an internally-tagged
 * enum, so the `type` field selects which of the other fields are present.
 *
 * Every field goes through the engine's `{{variable}}` substitution, so
 * secrets belong in an environment rather than in the request itself.
 */
export type AuthScheme =
  | { type: "bearer"; token: string }
  | { type: "basic"; username: string; password: string }
  | { type: "api_key"; name: string; value: string; location: ApiKeyLocation }
  | {
      type: "oauth2_client_credentials";
      token_url: string;
      client_id: string;
      client_secret: string;
      scope?: string | null;
    }
  | {
      type: "oauth2_authorization_code";
      auth_url: string;
      token_url: string;
      client_id: string;
      client_secret: string;
      scope?: string | null;
    }
  | { type: "digest"; username: string; password: string };

/** Every `AuthScheme` tag, plus the "no auth at all" case the UI needs. */
export type AuthSchemeType = AuthScheme["type"];

export interface NovaEnvironment {
  name: string;
  variables: Record<string, string>;
  /** Names of `variables` flagged as secret — masked behind a reveal toggle in the environment editor. */
  secrets: string[];
  auth: AuthScheme | null;
  path: string;
}

/** Every protocol a `.nova` request file can declare via `[request]`'s `protocol:` line. */
export type RequestProtocol = "http" | "websocket" | "sse";

export interface RequestFile {
  name: string;
  path: string;
  /** This request's HTTP method (e.g. `"GET"`), for the sidebar's method badge. Empty if unparseable, or if `protocol` isn't `"http"`. */
  method: string;
  /**
   * This request's `[request]` `protocol:` line — `"http"` (the default,
   * for a file with no explicit `protocol:` at all), `"websocket"`, or
   * `"sse"`. Picks which panel component/badge the GUI shows for this
   * request.
   */
  protocol: RequestProtocol;
}

export interface Collection {
  name: string;
  path: string;
  children: Collection[];
  requests: RequestFile[];
}

export interface NovaProject {
  root: string;
  manifest: Manifest;
  environments: NovaEnvironment[];
  /** Absolute path to the project's environments directory (e.g. `<nova-root>/envs`). */
  environments_dir: string;
  collections: Collection;
}

/** Mirrors `nova_engine::GitFileStatus`. */
export type GitFileStatus =
  | "untracked"
  | "unstaged"
  | "staged"
  | "committed"
  | { renamed: { from: string } };

/** The status "kind" regardless of whether it's a plain string or `Renamed`'s object shape. */
export function gitStatusKind(status: GitFileStatus): "untracked" | "unstaged" | "staged" | "committed" | "renamed" {
  return typeof status === "string" ? status : "renamed";
}

/**
 * Per-file git status for a project, keyed by absolute path (matching
 * `RequestFile.path`/`Collection.path` exactly) — only non-clean files are
 * present, so a missing entry means "committed"/clean. `null` when the
 * project isn't inside a git repository at all.
 */
export type GitStatusMap = Record<string, GitFileStatus>;

/**
 * Mirrors `nova_engine::OpenProjectOutcome` — opening a directory that
 * simply has no project in it is not an error, so the UI can offer to
 * create one there. Anything genuinely broken still rejects.
 */
export type OpenProjectOutcome = { found: NovaProject } | "not_found";

/** Mirrors `nova_engine::GitignoreOutcome`. */
export type GitignoreOutcome = "created" | "appended" | "already_present";

/** Mirrors `nova_engine::HookOutcome`; each variant carries the hook's path. */
export type HookOutcome = { installed: string } | { already_installed: string };

/**
 * Mirrors `nova_engine::InitOutcome`. `hook` is null when no hook was
 * asked for, and otherwise a serialized Rust `Result` — a failed hook
 * install doesn't fail the init, since the project files are already
 * written by then.
 */
export interface InitOutcome {
  project_root: string;
  gitignore: GitignoreOutcome;
  hook: { Ok: HookOutcome } | { Err: string } | null;
}

export interface ResponseHeader {
  name: string;
  value: string;
}

/**
 * Mirrors `nova_engine::execution::http::ResponseTiming` — the phase
 * breakdown backing the response pane's Timeline tab (#165).
 *
 * `ureq` (the HTTP client `nova-engine` uses) doesn't expose DNS lookup /
 * TCP connect / TLS handshake as separate, hookable phases, so those aren't
 * split out here. Only two phases are genuinely measured:
 * `time_to_first_byte_ms` bundles DNS + connect + TLS + sending the request
 * + waiting on the server (everything up through the response head
 * arriving), and `content_download_ms` is the time spent reading the body
 * afterward. `time_to_first_byte_ms + content_download_ms` equals the
 * response's `elapsed_ms`.
 */
export interface ResponseTiming {
  time_to_first_byte_ms: number;
  content_download_ms: number;
}

/** Mirrors `nova_engine::execute::Response`. */
export interface RequestResponse {
  status: number;
  headers: ResponseHeader[];
  body: string;
  elapsed_ms: number;
  timing: ResponseTiming;
}

/** Mirrors `nova_engine::Header`. */
export interface RequestHeader {
  name: string;
  value: string;
}

/** Mirrors `nova_engine::QueryParam`. */
export interface QueryParam {
  name: string;
  value: string;
}

/**
 * Mirrors `nova_engine::MultipartField` — a single part of a
 * `multipart/form-data` body. A field's content is either typed in by hand
 * (`value`) or attached from a file on disk (`file_path`, a path relative
 * to the project root whose bytes are read at send time); when `file_path`
 * is set, `value` is empty and ignored.
 */
export interface MultipartField {
  name: string;
  filename: string | null;
  content_type: string | null;
  value: string;
  file_path: string | null;
}

/** Mirrors `nova_engine::GraphQlBody`. */
export interface GraphQlBody {
  query: string;
  variables: unknown | null;
  operation_name: string | null;
}

/** Mirrors `nova_engine::GraphQlArgDef`. */
export interface GraphQlArgDef {
  name: string;
  description: string | null;
  type_ref: string;
}

/** Mirrors `nova_engine::GraphQlFieldDef`. */
export interface GraphQlFieldDef {
  name: string;
  description: string | null;
  args: GraphQlArgDef[];
  type_ref: string;
}

/** Mirrors `nova_engine::GraphQlTypeDef`. */
export interface GraphQlTypeDef {
  name: string;
  kind: string;
  description: string | null;
  fields: GraphQlFieldDef[];
}

/**
 * Mirrors `nova_engine::GraphQlSchema` — the result of introspecting a
 * GraphQL server, as fetched via `fetchGraphqlSchema`. `query_type`/
 * `mutation_type`/`subscription_type` name the root operation type (if the
 * server declares one); look it up in `types` to get its field list.
 */
export interface GraphQlSchema {
  query_type: string | null;
  mutation_type: string | null;
  subscription_type: string | null;
  types: GraphQlTypeDef[];
}

/**
 * Mirrors `nova_engine::RequestDraft` — a flattened, GUI-editable view of a
 * `.nova` file's method/URL/query/headers/body. The body always comes back
 * as plain text (the engine reduces whatever body shape the file declares
 * — JSON/XML/form/multipart/text — down to text via
 * `RequestBody::to_body_text`); saving sends that text straight back and
 * the engine re-infers the body's shape from the (possibly also edited)
 * `Content-Type` header via `RequestBody::from_text`, exactly like parsing
 * a `.nova` file from scratch does.
 *
 * `auth` is the request's `[auth]` section (null when it declares none),
 * and `sync_content_type` is its `[settings]` toggle; both round-trip
 * through save exactly like the other editable fields.
 *
 * `assert_text` is the `[assert]` section's assertion/extraction lines,
 * verbatim (comments and interleaving between the two kinds aren't
 * preserved — extractions are always re-emitted before assertions on
 * save, same as a malformed line is rejected at save time, not silently
 * dropped). `script_pre`/`script_post` are the `[script]` section's
 * `pre:`/`post:` names. `has_example_response`/`example_responses` aren't
 * editable here — they just let the GUI say what example responses the
 * file already has; saving always preserves them unchanged.
 */
/**
 * The built-in interpreter a `[script]` `pre:`/`post:` reference's file
 * extension maps to (`.js`/`.mjs`/`.ts` -> `"javascript"`, `.py` ->
 * `"python"`) — mirrors the engine's `interpreter_for`. The Scripts tab
 * editor uses this to decide whether a script gets syntax highlighting and
 * lint/beautify, or falls back to plain text for a custom/external
 * interpreter mapping the engine has no built-in extension for (bash/shell
 * included — see `ScriptLanguage` in `execution/script.rs` for why it's
 * not a built-in language yet).
 */
export type ScriptLanguage = "javascript" | "python";

export interface RequestDraft {
  method: string;
  url: string;
  query: QueryParam[];
  headers: RequestHeader[];
  body_text: string;
  auth: AuthScheme | null;
  /**
   * Whether picking a body type also rewrites the `Content-Type` header.
   * Defaults to true; a request turns it off to manage `Content-Type`
   * entirely by hand.
   */
  sync_content_type: boolean;
  assert_text: string;
  script_pre: string | null;
  script_post: string | null;
  has_example_response: boolean;
  /**
   * Every example response the file declares, summarized for the response
   * pane's example picker — empty when there are none, a single entry for
   * a classic one-example file. See {@link ExampleResponseSummary}.
   */
  example_responses: ExampleResponseSummary[];
}

/**
 * Mirrors `nova_engine::ExampleResponseSummary` — enough to label one entry
 * in the response pane's example picker (e.g. "200 OK" or "404
 * not_found") without shipping every example's full headers/body just to
 * populate a dropdown.
 */
export interface ExampleResponseSummary {
  status: number;
  name: string | null;
}

/**
 * Mirrors `nova_engine::WebSocketMessage` — one entry in a WebSocket
 * request's `[messages]` section: either a plain text frame, or a
 * reference to a file (path relative to the project root) whose raw bytes
 * are sent as a single binary frame.
 */
export type WebSocketMessage =
  | { kind: "text"; text: string }
  | { kind: "binary_file"; path: string };

/**
 * Mirrors `nova_engine::WebSocketDraft` — a flattened, GUI-editable view of
 * a `.nova` WebSocket connection declaration (`protocol: websocket`): the
 * URL, headers, and the ordered list of messages sent once connected.
 * There's no method/query/body/auth/assertions/example response for a
 * WebSocket request — see `docs/reference/nova-file-format.md`'s
 * "WebSocket requests" section for the full shape.
 */
export interface WebSocketDraft {
  url: string;
  headers: RequestHeader[];
  messages: WebSocketMessage[];
}

/**
 * Mirrors `nova_engine::WebSocketReceivedMessage` — one message received
 * over a WebSocket connection. A binary frame's bytes come back
 * base64-encoded (JSON has no native byte string) rather than rendered as
 * text.
 */
export type WebSocketReceivedMessage =
  | { kind: "text"; text: string }
  | { kind: "binary"; data_base64: string; len: number };

/**
 * Mirrors `nova_engine::WebSocketExchange` — the result of connecting to a
 * WebSocket request's URL, sending its declared messages in order, and
 * collecting whatever came back before the read timeout elapsed or the
 * server closed the connection.
 */
export interface WebSocketExchange {
  sent: WebSocketMessage[];
  received: WebSocketReceivedMessage[];
  elapsed_ms: number;
}

/**
 * Mirrors `nova_app_lib::websocket_session::WebSocketSessionStatus` —
 * whether the desktop app's interactive WebSocket panel currently has an
 * open session, for a tab to reflect connection state on reopen/reload
 * without needing to reconnect to find out.
 */
export interface WebSocketSessionStatus {
  connected: boolean;
}

/**
 * Mirrors `nova_app_lib::websocket_session::WsMessageEvent` — the payload
 * of a `"ws-session:message"` event, emitted once per message the
 * currently-open interactive WebSocket session receives, in arrival order.
 * `text` is set for a text frame; `dataBase64`/`len` are set for a binary
 * one — never both.
 */
export interface WsSessionMessageEvent {
  text: string | null;
  dataBase64: string | null;
  len: number | null;
  atMs: number;
}

/**
 * Mirrors `nova_engine::ParsedCurlRequest` — the pieces recovered from a
 * pasted `curl`/`wget` command. No `query`: a raw curl URL isn't split
 * into base/query the way `RequestDraft` is, so the caller runs it through
 * the same URL-field parsing used for a manually-typed query string.
 */
export interface ParsedCurlRequest {
  method: string;
  url: string;
  headers: RequestHeader[];
  body: string | null;
}

/**
 * Mirrors `nova_engine::ExportFormat` — the target [`exportRequestAs`]
 * renders a resolved request as. Values match the engine's
 * `#[serde(rename_all = "snake_case")]` wire format.
 */
export type ExportFormat = "curl" | "fetch";

/**
 * Mirrors `nova_engine::AssertionOutcome` — one `[assert]` line's result.
 * `failure` is set only when `passed` is false: what was expected vs. what
 * was actually found, specific enough to act on.
 */
export interface AssertionOutcome {
  raw: string;
  passed: boolean;
  failure: string | null;
}

/**
 * Mirrors `nova-app`'s `commands::TestRequestResult` — one request's
 * outcome from a "Run Tests" pass. Either it ran (`response`/`outcomes`
 * populated, `error` null) or it failed outright — couldn't be parsed,
 * resolved, or sent (`error` set, `response` null, `outcomes` empty).
 */
export interface TestRequestResult {
  path: string;
  method: string;
  url: string;
  response: RequestResponse | null;
  outcomes: AssertionOutcome[];
  error: string | null;
}

/**
 * Mirrors `nova-app`'s `commands::TestRunResult` — the result of running
 * every request under a project (or a single collection/request within it)
 * as a test via the `run_tests` Tauri command.
 */
export interface TestRunResult {
  passed: number;
  failed: number;
  requests: TestRequestResult[];
}

/**
 * Mirrors `nova-app`'s `commands::ResolvedVariables` — the resolved
 * `{{name}}` -> value map `get_resolved_variables` returns, alongside the
 * names among them that the active environment flags secret. Powers the
 * request panel's read-only variables drawer, which masks a row whose name
 * appears in `secrets` the same way the environment editor masks its own
 * secret-flagged rows.
 */
export interface ResolvedVariables {
  variables: Record<string, string>;
  secrets: string[];
}

/**
 * Mirrors `nova-app`'s `commands::HistorySummary` — one past send reduced
 * to what a history list needs to show (method/status/timing/timestamp
 * plus the URL). Fetch the full request/response with `reopenHistoryEntry`
 * when a row is clicked, rather than shipping every stored response body
 * up front.
 */
export interface HistorySummary {
  id: number;
  method: string;
  url: string;
  status: number;
  elapsed_ms: number;
  /** Milliseconds since the Unix epoch — format with `new Date(sent_at_ms)`. */
  sent_at_ms: number;
}

/**
 * Mirrors `nova-app`'s `commands::HistoryDetail` — one history entry
 * reopened in full: the request it recorded (as a `RequestDraft`, the same
 * shape the request panel already renders) alongside the response that
 * came back.
 */
export interface HistoryDetail {
  request: RequestDraft;
  response: RequestResponse;
}

/**
 * Mirrors `nova_engine::session::CookieView` — one cookie currently stored
 * in a project's session, flattened for display/editing. Already-expired
 * cookies are never included — see `getCookies`.
 */
export interface CookieView {
  host: string;
  name: string;
  value: string;
  path: string;
  secure: boolean;
  domain: string | null;
  /** Milliseconds since the Unix epoch — `null` means no expiry (lasts for the life of the session). */
  expires_at_ms: number | null;
}

/** Mirrors `nova_engine::diff::StatusDiff` — a status code that changed between the two sides of a diff. */
export interface StatusDiff {
  before: number;
  after: number;
}

/**
 * Mirrors `nova_engine::diff::HeaderChange` — one header that differs
 * between the two sides of a diff, tagged by `kind` (a Rust
 * `#[serde(tag = "kind")]` enum): added outright, removed outright, or
 * present on both sides with a different value.
 */
export type HeaderChange =
  | { kind: "Added"; name: string; value: string }
  | { kind: "Removed"; name: string; value: string }
  | { kind: "Changed"; name: string; before: string; after: string };

/**
 * Mirrors `nova_engine::diff::JsonChange` — one JSON value that differs
 * between the two sides of a diff, addressed by a `jq`-style path from the
 * document root (e.g. `$.user.id`). `value`/`before`/`after` are the raw
 * JSON values (any shape), passed through as-is for the caller to render.
 */
export type JsonChange =
  | { kind: "Added"; path: string; value: unknown }
  | { kind: "Removed"; path: string; value: unknown }
  | { kind: "Changed"; path: string; before: unknown; after: unknown };

/** Mirrors `nova_engine::diff::TextDiffLine` — one line of a line-based body diff. */
export type TextDiffLine =
  | { kind: "Added"; line: string }
  | { kind: "Removed"; line: string }
  | { kind: "Unchanged"; line: string };

/**
 * Mirrors `nova_engine::diff::BodyDiff` — the body half of a
 * `ResponseDiff`. `Json` is used when both sides parse as JSON (a
 * structural, path-addressed diff); `Text` is the line-based fallback for
 * everything else; `Unchanged` means the two bodies were byte-for-byte
 * identical.
 */
export type BodyDiff =
  | { kind: "Json"; changes: JsonChange[] }
  | { kind: "Text"; lines: TextDiffLine[] }
  | { kind: "Unchanged" };

/**
 * Mirrors `nova_engine::diff::ResponseDiff` — the result of comparing two
 * responses (see `diffAgainstPreviousRun`/`diffAgainstExampleResponse`).
 * `status` is null when the status code didn't change; `identical` is a
 * convenience that's true only when status, headers, and body all compare
 * equal.
 */
export interface ResponseDiff {
  status: StatusDiff | null;
  header_changes: HeaderChange[];
  body: BodyDiff;
  identical: boolean;
}

/**
 * Mirrors `nova-app`'s `commands::ImportProjectOutcome` — the result of
 * generating a new Nova project from an OpenAPI spec or Postman collection
 * export via the `import_project` Tauri command.
 */
export interface ImportProjectOutcome {
  project_root: string;
  request_count: number;
  warnings: string[];
}

/**
 * Mirrors `nova-app`'s `mock_server::MockServerStatus` — the desktop app's
 * mock server toggle state. `host`/`port` are set only while `running` is
 * true.
 */
export interface MockServerStatus {
  running: boolean;
  host: string | null;
  port: number | null;
}

/**
 * Mirrors `nova-engine`'s `mock::MockCallLogEntry` — one request the
 * running mock server handled, for the desktop app's mock server call log.
 */
export interface MockCallLogEntry {
  id: number;
  received_at_ms: number;
  method: string;
  path: string;
  matched_route: string | null;
  status: number;
}
