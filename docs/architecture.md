# Architecture

Nova is a Cargo workspace built around one rule: **the engine owns the
product; the CLI and GUI are thin interfaces over it.**

```text
                 Nova Project
                      │
                 nova.yaml
                      │
                Nova Engine
               /           \
              /             \
        Nova CLI         Nova Desktop
```

## `nova-engine`

The core library. Everything that touches a project's files lives here and
nowhere else:

- discovering a project (`nova/nova.yaml`, walking upward like `git` looks
  for `.git`)
- parsing/serializing the manifest, environments, and `.nova` request files
- `{{variable}}` resolution
- HTTP (and WebSocket/SSE/gRPC) execution
- authentication schemes
- assertions and request chaining
- the local mock server
- OpenAPI and Postman-collection-format import/export
- curl/wget command parsing
- project validation, including a hardcoded-credential scan

Every public type derives `Serialize`, so both interfaces below can hand
engine results straight out (as CLI `--json` output, or to the desktop
app's frontend) without a parallel DTO layer.

## `nova-cli`

A thin `clap`-based CLI. Every subcommand is a few lines that call into
`nova-engine` and format the result — no parsing or execution logic of its
own. See the [CLI reference](reference/cli.md).

## `nova-app`

A Tauri desktop app: a Rust backend over a Vue 3 + TypeScript frontend. Its
Tauri commands are the same shape as the CLI's subcommands — thin wrappers
around the engine. See the [desktop app reference](reference/gui.md).

## The enforced boundary

Neither `nova-cli` nor `nova-app` ever parses a `.nova`/`nova.yaml`/
environment file, walks the collections directory, or resolves
`{{variable}}`s itself — that always goes through `nova-engine`. This is
what keeps the CLI and GUI behaviorally identical: a request runs the same
way whether it's sent from a terminal, a script, CI, or a button in the
desktop app.
