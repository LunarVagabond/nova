# CLI reference

`nova-cli` is a thin `clap`-based wrapper: every subcommand below just calls
into `nova-engine` and formats the result. No parsing, discovery, or
execution logic lives in the CLI crate itself — see
`crates/nova-cli/src/cli.rs` for the authoritative argument definitions.

Every command that takes a `path` defaults to `.` and works from anywhere
inside a project (project discovery walks upward to find `nova/nova.yaml`).

```bash
nova [path]                    # shorthand for `nova inspect [path]`

nova init [path]                # scaffold a new project
nova open [path]                 # print project structure (same as bare `nova [path]`)
nova inspect [path]              # print manifest, environments, and collection tree

nova validate [path]             # validate manifest/environments/requests

nova run <request>               # execute a single .nova file, or a directory of them
nova test [path]                 # run requests as assertions/tests
nova ws <request>                # open a WebSocket connection declared by a .nova file

nova generate <input> <output>   # OpenAPI spec or Postman export -> new Nova project
nova export [path]               # collections -> OpenAPI 3.x spec (YAML)

nova mock [path]                 # start a local mock server

nova check-secrets [path]        # scan for possible hardcoded credentials
nova install-hook [path]         # install a git pre-commit hook running check-secrets --staged

nova new request <name> [path]      # scaffold a new .nova file
nova new collection <name> [path]   # scaffold a new collection subdirectory
nova new environment <name> [path]  # scaffold a new environment file
```

## `nova init`

Scaffolds `nova/nova.yaml`, an empty `nova/collections/`, and a starter
`nova/envs/` with one example environment. Adds `nova/envs/` to
`.gitignore`. Refuses to overwrite an existing `nova/` directory.

Run interactively, it prompts for anything not given as a flag (project
name, whether to install the pre-commit hook). Run non-interactively (CI, a
script, piped input) it never prompts and falls back to the flag defaults.

- `--name <name>` — project name for the manifest; defaults to the target
  directory's name, and skips the interactive name prompt.
- `--with-hook` — also install the pre-commit hook (see `install-hook`).
- `--no-hook` — explicitly skip it (the default); given explicitly, skips
  the interactive hook prompt too. Conflicts with `--with-hook`.

## `nova open` / bare `nova [path]`

Print a project's structure. Identical to `nova inspect`.

## `nova inspect`

Load a project and print its manifest, environments, and collection tree —
useful for confirming the engine is discovering a project correctly.

## `nova validate`

Validate project layout, manifest, environments, and request files. See
[project-structure.md](./project-structure.md#validation) for exactly what's
checked.

## `nova run <request>`

Execute a single `.nova` file, or every request under a directory.

- `--environment <name>` — environment to resolve `{{variable}}`s against;
  defaults to the manifest's `defaults.environment`.

## `nova test [path]`

Run requests and evaluate their `[assert]` sections as pass/fail tests — the
same assertion engine `nova run` uses, just treated as a test suite rather
than one-off execution.

- `--environment <name>`

## `nova ws <request>`

Open the WebSocket connection a `.nova` file declares (`protocol: websocket`
under `[request]` — see [nova-file-format.md](./nova-file-format.md#websocket-requests)),
send its declared `[messages]`, and print what comes back. Like `nova run`,
successfully connecting and exchanging messages counts as success regardless
of what those messages actually were — only a failure to parse the request,
resolve its `{{variable}}`s, or open/use the connection at all is a CLI
failure.

- `--environment <name>`
- `--message <text>` — an additional message to send, after any declared in
  `[messages]`. Repeatable; sent in the order given.
- `--timeout-secs <n>` — how long to keep waiting for another message after
  the last one (or after connecting, if there's nothing to send) before
  giving up and closing the connection. Default `5`.

## `nova generate <input> <output>`

Generate a Nova project from an OpenAPI 3.x spec (YAML or JSON) or a Postman
Collection Format v2.1 export (JSON) — the format is auto-detected from the
file's contents. `output` is the directory a new `nova/` gets created
inside.

## `nova export [path]`

Export a project's collections as an OpenAPI 3.x spec (YAML), printed to
stdout by default.

- `--output <file>` — write to a file instead.

## `nova mock [path]`

Start a local mock server. For every `.nova` request found under `path`,
registers a route matching its method and path; a request with a
`[response <status>]` section serves that example verbatim, one without
still gets a route but always answers `501`. Mocking is static — nothing a
mocked request does affects a later mocked request's response.

- `--host <addr>` — default `127.0.0.1`.
- `--port <port>` — default `4010`; `0` picks any available port.

## `nova check-secrets [path]`

Check every request for a possible hardcoded credential (an `[auth]` field
or `Authorization` header with no `{{variable}}` reference at all — same
check as `nova validate`). Exits non-zero if any are found.

- `--staged` — only check `.nova` files currently staged in git, rather than
  the whole project. This is what the pre-commit hook actually runs, so an
  unrelated pre-existing issue elsewhere never blocks an unrelated commit.

## `nova install-hook [path]`

Install a git pre-commit hook that runs `check-secrets --staged` before
every commit. Opt-in, never installed automatically by anything but this
command (or `nova init --with-hook`). Appends to an existing pre-commit hook
rather than overwriting it, and is a no-op if already installed.

## `nova new`

Scaffold a new request, collection, or environment — the same engine
functions the desktop app's own "new" actions call, so CLI- and GUI-created
files are identical.

- `nova new request <name> [path] [--collection <subdir>]` — a new `.nova`
  file with a minimal default request (`GET {{base_url}}/`). `--collection`
  places it in a subdirectory of the collections root instead of the root
  itself.
- `nova new collection <name> [path] [--parent <subdir>]` — a new, empty
  collection subdirectory. `--parent` nests it inside another collection.
- `nova new environment <name> [path]` — a new environment file with no
  variables set.
