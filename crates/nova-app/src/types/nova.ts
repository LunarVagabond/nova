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
    };

/** Every `AuthScheme` tag, plus the "no auth at all" case the UI needs. */
export type AuthSchemeType = AuthScheme["type"];

export interface NovaEnvironment {
  name: string;
  variables: Record<string, string>;
  auth: AuthScheme | null;
  path: string;
}

export interface RequestFile {
  name: string;
  path: string;
  /** This request's HTTP method (e.g. `"GET"`), for the sidebar's method badge. Empty if unparseable. */
  method: string;
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

/** Mirrors `nova_engine::execute::Response`. */
export interface RequestResponse {
  status: number;
  headers: ResponseHeader[];
  body: string;
  elapsed_ms: number;
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
 * `has_assertions`/`has_extractions`/`has_example_response` aren't
 * editable here — they just let the GUI say a file has more to it than
 * this panel shows. Saving always preserves those sections unchanged.
 */
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
  has_assertions: boolean;
  has_extractions: boolean;
  has_example_response: boolean;
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
