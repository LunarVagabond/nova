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
nova sweep <request>              # resend a request once per value across one position
nova ws <request>                # open a WebSocket connection declared by a .nova file
nova sse <request>                # open an SSE connection declared by a .nova file
nova grpc <request>               # make the unary gRPC call declared by a .nova file

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
- `--save-example` — capture each request's response into its file's
  `[response <status>]` section, replacing whatever example response (if
  any) was already there. Running a directory saves one for every request
  in it. See [nova-file-format.md](./nova-file-format.md#response-status)
  for what that section is and how `nova mock` uses it.
- `--data <file.csv|file.json>` — run each request once per row/object in
  this file, with that iteration's columns/fields available as
  `{{variable}}`s layered on top of the active environment for just that
  one send (an iteration value wins over a same-named environment one). A
  CSV file's first row is its column headers; a JSON file is a flat array
  of objects. With `--save-example`, only the last iteration's response
  ends up saved, since each iteration writes the same `[response <status>]`
  section in turn.

## `nova test [path]`

Run requests and evaluate their `[assert]` sections as pass/fail tests — the
same assertion engine `nova run` uses, just treated as a test suite rather
than one-off execution.

- `--environment <name>`
- `--data <file.csv|file.json>` — run each request's assertions once per
  row/object in this file, the same as `nova run`'s `--data`. `passed`/
  `failed` totals across every iteration.

## `nova sweep <request>`

Resend a request once per value across one position — a query param, a
header, or a JSON body field — reporting status/elapsed time/response size
per variant against the unmodified baseline (variant zero), with anomalies
flagged: an unexpected 5xx (baseline succeeded, variant returned >= 500), a
timing outlier (a variant taking a lot longer than the baseline), or the
response's JSON shape changing at the top level. See
[nova-file-format.md](./nova-file-format.md#sweep) for the `.nova` `[sweep]`
section this reads by default, and #138 for the full design rationale
(bounded scope — "does my API handle this range of inputs sanely," not a
vulnerability scanner).

- `--environment <name>`
- `--position <spec>` — override the request's own `[sweep]` position (a
  `param:<name>`, `header:<name>`, or `body:<dotted.path>` spec).
- `--values <a,b,c>` / `--values-file <path>` / `--generator <names|all>` —
  override the request's own `[sweep]` values with, respectively, an inline
  comma-separated list, a project-root-relative values file (one value per
  line), or one or more built-in boundary-value generator names (`empty`,
  `very_long`, `negative`, `zero`, `huge`, `unicode`, `missing`), or `all`
  for every one of them.

  Giving any of `--position`/`--values`/`--values-file`/`--generator`
  replaces the request's `[sweep]` section entirely for that run — a
  partial override isn't supported. A request with no `[sweep]` section at
  all needs both `--position` and one of the value-source flags.

Exits non-zero if any variant is flagged with an anomaly, or if the request
can't be parsed/resolved/sent at all. `--json` prints the full report
(baseline, each variant's value/status/elapsed time/size/anomalies, and an
`anomaly_count` total) instead of a summary line per variant.

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

## `nova sse <request>`

Open the Server-Sent Events connection a `.nova` file declares (`protocol:
sse` under `[request]` — see
[nova-file-format.md](./nova-file-format.md#server-sent-events-requests)),
and print each event as it arrives rather than waiting for the connection
to close. Like `nova ws`, successfully connecting and streaming counts as
success regardless of what events actually arrived — only a failure to
parse the request, resolve its `{{variable}}`s, or open the connection at
all is a CLI failure.

- `--environment <name>`
- `--timeout-secs <n>` — how long to keep waiting for another event after
  the last one (or after connecting, if none have arrived yet) before
  giving up and closing the connection. Default `5`.

## `nova grpc <request>`

Make the unary gRPC call a `.nova` file declares (`protocol: grpc` under
`[request]` — see
[nova-file-format.md](./nova-file-format.md#grpc-requests)): compile its
named `.proto` file, encode `[body]`'s JSON against the call's input
message type, make the call, and print the response decoded back to JSON.
Like `nova ws`/`nova sse`, a call that connects and gets a response back
counts as success regardless of that response's own contents (a gRPC error
status included) — only a failure to parse the request, resolve its
`{{variable}}`s, compile the `.proto`, resolve `rpc` against it, or make
the call at all is a CLI failure. `--json` prints the full
`{ rpc, response, elapsed_ms }` result as JSON instead of the two-line
human summary.

- `--environment <name>`
- `--timeout-secs <n>` — how long to allow the whole call (connecting plus
  the round trip) before giving up. Default `10`.

## `nova generate <input> <output>`

Generate a Nova project from an OpenAPI 3.x spec (YAML or JSON) or a Postman
Collection Format v2.1 export (JSON) — the format is auto-detected from the
file's contents. `output` is the directory a new `nova/` gets created
inside.

## `nova export [path]`

Export a project's collections as an OpenAPI 3.x spec (YAML), printed to
stdout by default.

- `--output <file>` — write to a file instead.

## `nova export-request <request>`

Render a single request, after `{{variable}}` substitution, as a
copy-pasteable `curl` command or code snippet — for handing a request to
someone who doesn't have Nova installed, or dropping one into a bug report
or script.

- `--environment <name>` — resolve against this environment instead of the
  project's default.
- `--as <curl|fetch>` — target format to render as. Defaults to `curl`.

A request whose `[auth]` uses an OAuth2 client credentials grant can't be
fully reproduced this way (it needs a live token exchange), so the
rendered command includes a comment noting that a bearer token still needs
to be filled in by hand.

## `nova mock [path]`

Start a local mock server. For every `.nova` request found under `path`,
registers a route matching its method and path; a request with one or more
`[response <status> "name"]` sections serves an example verbatim, one
without any still gets a route but always answers `501`. Mocking is static —
nothing a mocked request does affects a later mocked request's response.

A request with more than one example serves its **lowest-status** example
by default (so a `200` and a `404` example on the same route answers `200`
for an ordinary request — this is also what a classic single-example file
already did). An incoming request can ask for a specific example instead
via two headers:

- `X-Nova-Mock-Example: <name>` — serve the example with this name.
- `X-Nova-Mock-Status: <status>` — serve the example at this status code.

`X-Nova-Mock-Example` takes priority when both are given; either one falls
through to the default lowest-status example if it doesn't match any
declared example, rather than answering `404`/`501`. See
[the `.nova` file format's `[response <status>]` section](./nova-file-format.md#response-status)
for how to declare multiple examples.

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
  file with a minimal default request (`GET {{base_url}}/`), its `[headers]`
  pre-populated with `User-Agent`/`Accept`/`Accept-Encoding` (the same values
  sent implicitly if a request omits them — see `nova inspect`/the engine's
  `execute` — but written here as ordinary editable/deletable rows instead).
  `--collection` places it in a subdirectory of the collections root instead
  of the root itself.
- `nova new collection <name> [path] [--parent <subdir>]` — a new, empty
  collection subdirectory. `--parent` nests it inside another collection.
- `nova new environment <name> [path]` — a new environment file with no
  variables set.
