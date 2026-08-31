# Quickstart

This is the fastest path from "never used Nova" to sending, mocking, and
testing requests. For the full command list or file-format details, see the
docs linked at the end of each section rather than duplicated here.

## 1. Getting the binaries

Every release ships three bundle combinations (Linux x86_64 for now), so you
only need to grab the one that matches what you want:

| I want... | Grab |
|---|---|
| Just the CLI (scripts, CI, terminal use) | `nova-cli-v<version>-linux-x86_64.tar.gz` or `nova-cli_<version>_amd64.deb` |
| Just the desktop app | `nova-gui_<version>_amd64.AppImage` or `nova-gui_<version>_amd64.deb` |
| Both, in one package | `nova-full_<version>_amd64.AppImage` or `nova-full_<version>_amd64.deb` |

Download from the project's [GitHub Releases](https://github.com/LunarVagabond/nova/releases)
page rather than pinning a version here.

Install mechanics by format:

- **AppImage**: `chmod +x nova-gui_*.AppImage && ./nova-gui_*.AppImage`
- **.deb**: `sudo apt install ./nova-gui_<version>_amd64.deb` (or
  `sudo dpkg -i nova-gui_<version>_amd64.deb`)
- **.tar.gz** (CLI only): extract it and put the `nova` binary somewhere on
  your `PATH`, e.g. `tar -xzf nova-cli-v<version>-linux-x86_64.tar.gz && sudo mv nova /usr/local/bin/`

## 2. Setting them up

Confirm the CLI is installed and on your `PATH`:

```bash
nova --version
```

If you installed a GUI or full bundle, launch the desktop app from your
application menu (or run the AppImage/binary directly). It's a Tauri app —
no separate backend to start.

## 3. Using it like Postman, if that's all you want

You don't need to care about the Git-native angle at all to get value out of
Nova. Inside a repo you already have:

```bash
nova init
```

This scaffolds a `nova/` directory (`nova.yaml`, an empty `nova/collections/`,
and a starter `nova/envs/` with one example environment) and refuses to
overwrite one that already exists. Run it interactively and it'll prompt you
for a project name and whether to install a pre-commit hook (more on that
below); run it non-interactively (CI, a script, piped input) and it just uses
the defaults without prompting.

From there, open the desktop app, point it at that project, and build/send
requests ad hoc through the Params/Auth/Headers/Body tabs — exactly like any
other API client. The file-based, Git-native model is a bonus you can grow
into later, not a tax you pay upfront.

## 4. Running things

A single request:

```bash
nova run nova/collections/users/create.nova
```

or a whole directory of them at once (`nova run nova/collections/`).

A local mock server, so you can develop against example responses before a
real backend exists:

```bash
nova mock
```

For every request under the project that has a `[response <status>]`
section, this serves that example verbatim on the matching method/path
(default `127.0.0.1:4010`; override with `--host`/`--port`).

A WebSocket, SSE, or gRPC connection, if a `.nova` file declares one
(`protocol: websocket`, `protocol: sse`, or `protocol: grpc` under
`[request]` — see the
[`.nova` file format reference](reference/nova-file-format.md#websocket-requests)
for the exact syntax):

```bash
nova ws nova/collections/chat/echo.nova
nova sse nova/collections/events/stream.nova
nova grpc nova/collections/greeter/say_hello.nova
```

Assertions, as a lightweight test suite:

```bash
nova test
```

This runs the same `[assert]` sections `nova run` evaluates, but treats
pass/fail as the point rather than a side effect.

Sweeping a range of inputs across one field, to see how an endpoint holds
up (an empty value, a huge one, a missing one, or your own list):

```bash
nova sweep nova/collections/users/create.nova
```

This reads the request's own `[sweep]` section (see the
[`.nova` file format reference](reference/nova-file-format.md#sweep)) and
reports each variant's status/timing/response size against the unmodified
baseline, flagging anomalies (an unexpected server error, a timing outlier,
or a changed response shape).

## 5. The optional pre-commit hook

Nova can install a git pre-commit hook that runs `nova check-secrets
--staged` before every commit, catching an `[auth]` field or `Authorization`
header that's a literal value instead of a `{{variable}}` reference — the
most common way a real API key or token ends up committed by accident. It
protects against that one specific mistake, not general security review, and
it's entirely opt-in: skip the prompt during `nova init`, or install it
later yourself:

```bash
nova install-hook
```

Nova's own `.gitignore` scaffolding from `nova init` also excludes
`nova/envs/`, since environment files are where local secrets (base URLs,
tokens, credentials) usually live day to day.

## 6. A few things you'll want soon

**Pointing a command at a project that isn't the current directory** — every
command that takes a `path` accepts one explicitly and defaults to `.`
otherwise, e.g. `nova run ../other-project/nova/collections/login.nova` or
`nova validate ~/code/some-api`. Project discovery walks upward from
whatever path you give it looking for `nova/nova.yaml`, so you can also just
run from any subdirectory of a project.

**Environments** — almost every real request references a `{{variable}}`
like `{{base_url}}` or `{{token}}`, resolved against one of the flat YAML
files under `nova/envs/` (`nova run`/`nova test` use
`defaults.environment` from `nova.yaml` unless you pass `--environment
<name>`). See [Project structure and `nova.yaml`](reference/project-structure.md#environments).

**Adopting Nova for an API you already have** — instead of hand-writing
requests from scratch, import an OpenAPI spec or a Postman Collection
export:

```bash
nova generate openapi.yaml .
```

The format is auto-detected from the file's contents; `output` is the
directory a new `nova/` gets created inside. See the
[CLI reference](reference/cli.md#nova-generate-input-output) for details, and
[project-structure.md](reference/project-structure.md) /
[nova-file-format.md](reference/nova-file-format.md) for everything else a
project or request can declare (multipart bodies, GraphQL, structured auth
schemes, request chaining, and more).

Stuck, or something doesn't work the way this guide says it should? See
[SUPPORT.md](../.github/SUPPORT.md) — GitHub Issues/Discussions, or the
project's Discord, are all fair game.
