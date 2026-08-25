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

export interface NovaEnvironment {
  name: string;
  variables: Record<string, string>;
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
  collections: Collection;
}
