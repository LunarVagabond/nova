# Project structure and `nova.yaml`

A Nova project is any directory containing a `nova/` directory with a
`nova.yaml` manifest inside it. `NovaProject::discover`
(`crates/nova-engine/src/project/mod.rs`) walks upward from a given path looking
for it, the same way `git` looks for `.git` — so any command can be run from
a subdirectory of the project, not just its root.

## Layout

```text
my-project/
├── src/
└── nova/
    ├── nova.yaml
    ├── collections/
    │   ├── auth/
    │   │   └── login.nova
    │   └── users/
    │       ├── create.nova
    │       └── get.nova
    ├── envs/
    │   ├── local.yaml
    │   └── staging.yaml
    ├── globals.yaml
    └── scripts/
        └── sign-request.py
```

- **`collections/`** (path configurable, see below) is walked recursively;
  any subdirectory is a collection, any `.nova` file inside one is a
  request. See `crates/nova-engine/src/project/collection.rs`.
- **`envs/`** (path configurable) is *not* walked recursively — every
  `*.yaml`/`*.yml` file directly inside it is one environment. See
  `crates/nova-engine/src/project/environment.rs`.
- **`scripts/`** (not configurable) holds pre-request/post-response
  scripts that a request's `[script]` section names by bare filename. See
  `crates/nova-engine/src/execution/script.rs` and the `.nova` file format
  reference.
- Individual requests are never listed in the manifest; they're always
  discovered from disk.

## `nova.yaml`

```yaml
version: 1

project:
  name: My API

defaults:
  environment: local

collections:
  path: collections

environments:
  path: envs
```

- `version` — manifest schema version. Currently always `1`; the engine
  refuses to load a manifest with any other value rather than guessing at
  compatibility.
- `project.name` — display name.
- `defaults.environment` — which environment `nova run`/`nova test` use when
  `--environment` isn't given. `nova validate` flags a `defaults.environment`
  that doesn't match any discovered environment file.
- `defaults.timeout` — optional, currently unused for wire-level enforcement
  beyond what the field carries.
- `collections.path` / `environments.path` — relative to `nova/`, default to
  `collections` and `envs` respectively. Changing them just changes where
  the engine looks; nothing else about the format changes.

## Environments

An environment file is a flat `name` + `variables:` map, plus an optional
`secrets:` list and an optional default `auth:` scheme:

```yaml
name: local

variables:
  base_url: http://localhost:8080
  token: dev-token-123

secrets:
  - token

auth:
  type: bearer
  token: "{{token}}"
```

`secrets` names which entries in `variables` hold sensitive values. It's a
display-only flag: nova-engine just carries the list through parsing and
serialization, and the desktop app's environment editor renders a flagged
variable's value masked behind a reveal toggle rather than in plain text.
An environment file with no `secrets:` key (every file written before this
existed, or one with nothing flagged) loads with an empty list, so existing
files keep working unchanged.

`nova init` gitignores a new project's `envs/` directory by default, since
environment files commonly hold local secrets. Where a secret should live
long-term beyond a gitignored environment file is still open — see
`nova check-secrets` / `nova install-hook` below for what exists today.
External secret-provider integration is a still-open idea, tracked as a
GitHub issue rather than documented here.

## Global variables

`globals.yaml` (path not configurable), directly inside `nova/`, is
optional and holds variables that apply project-wide, independent of the
active environment or which collection a request lives in:

```yaml
variables:
  api_version: v2
  default_timeout: "30"
```

Missing entirely is the same as an empty `variables:` map — nothing has
to opt in. See `crates/nova-engine/src/project/globals.rs`.

A collection can also hold its own variables, scoped to requests directly
inside that directory (not inherited by subcollections), in a
`_collection.yaml` file next to its requests:

```yaml
variables:
  base_path: /api/v1
```

**Variable resolution precedence**, lowest to highest: `globals.yaml`,
then the owning collection's `_collection.yaml`, then any variable a
running session has extracted from an earlier response (`[assert]`
extractions, `[script]` `post:` hooks), then the active environment's own
`variables:`. Each layer only fills in names the layers above it don't
already define — see `NovaProject::effective_collection_variables` and
`Session::resolve_and_execute_in_collection`.

## Validation

`nova validate` (CLI and desktop app, same engine call — `validate.rs`)
checks:

- `defaults.environment` names a real environment
- no two environments share a name
- the project isn't empty (no environments and no requests)
- every request's `[auth]` field or `Authorization` header actually
  references a `{{variable}}` — a literal value there is flagged as a
  possible hardcoded credential, since request files are always committed
  while environment files usually aren't

`nova check-secrets [--staged]` runs just the hardcoded-credential half of
that scan on its own, and is what `nova install-hook`'s pre-commit hook
invokes against staged files before every commit.

## Related

- [The `.nova` request file format](./nova-file-format.md)
- [CLI reference](./cli.md)
