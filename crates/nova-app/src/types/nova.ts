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

export interface AuthDefault {
  header: string;
  value: string;
}

export interface NovaEnvironment {
  name: string;
  variables: Record<string, string>;
  auth: AuthDefault | null;
  path: string;
}

export interface RequestFile {
  name: string;
  path: string;
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
 * Mirrors `nova_engine::RequestDraft` — a flattened, GUI-editable view of a
 * `.nova` file's method/URL/query/headers/body. The body always comes back
 * as plain text (the engine reduces whatever body shape the file declares
 * — JSON/XML/form/multipart/text — down to text via
 * `RequestBody::to_body_text`); saving sends that text straight back and
 * the engine re-infers the body's shape from the (possibly also edited)
 * `Content-Type` header via `RequestBody::from_text`, exactly like parsing
 * a `.nova` file from scratch does.
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
