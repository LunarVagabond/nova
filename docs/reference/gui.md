# Desktop app (GUI)

`nova-app` is a Tauri desktop app: Rust backend (`src-tauri/`) over a Vue 3 +
TypeScript frontend (`src/`). It's a visual interface over the same
`nova-engine` the CLI uses — it never parses `.nova`/`nova.yaml`/environment
files itself; see the enforced-boundary rule below.

Run it in dev mode with `make dev` (installs frontend deps first); build a
release bundle with `make build-app`.

## Layout

- **`src-tauri/src/commands.rs`** — every Tauri command. Each is a couple of
  lines that call `nova-engine` and map `NovaError` to `String` (or a
  richer typed result, e.g. `OpenProjectOutcome`) at the boundary.
- **`src-tauri/src/session_store.rs`** — Tauri-managed state holding one
  `nova_engine::Session` per open project root, keyed by project root, so
  cookies/chained variables/request history accumulate across separate Send
  clicks in the same project instead of a fresh `Session` resetting them on
  every command call.
- **`src-tauri/src/mock_server.rs`** — lifecycle management for the mock
  server toggle in the top bar: binds a `tiny_http::Server` on a background
  thread and holds its handle in Tauri-managed state (`MockServerState`) so
  it survives across command invocations and stops cleanly on explicit
  stop or app exit. The route table itself (which `.nova` requests become
  which routes) still comes from `nova_engine::mock`, shared with `nova
  mock` — only the "actually run an HTTP server and keep a handle to it" is
  GUI-side, since that's inherently desktop-app state, not engine state.
- **`src/types/nova.ts`** — hand-mirrors the engine's `Serialize` shapes.
- **`src/api/nova.ts`** — wraps `invoke()` and the dialog plugin.
- **`src/components/`** — `Sidebar.vue`/`ProjectPanel.vue`/`CollectionNode.vue`
  for the collection tree; `RequestPanel.vue` for the HTTP request editor,
  split into Params/Auth/Headers/Body tabs (`AuthEditor.vue`,
  `KeyValueEditor.vue`, `MultipartEditor.vue`, `CodeEditor.vue`);
  `WebSocketPanel.vue` for a WebSocket request (`protocol: websocket`) — a
  separate component from `RequestPanel.vue` rather than another tab on it,
  since a WebSocket request has no method/params/body/auth/assertions/
  example response to begin with (see below); `EnvironmentPanel.vue` for
  environment editing; `HistoryPanel.vue` for the current project's recent
  sends (method/status/timing/timestamp), reachable from the top bar's
  clock-icon action, with a click on an entry reopening its full stored
  request/response — in-memory and per-session, so it resets when the app
  restarts; `ResponseDiffView.vue` renders a structured response diff (see
  below) for the response pane's Diff tab; `Modal.vue` is the shared
  in-app dialog component — used instead of `window.prompt`/
  `window.confirm`, which are unreliable inside Tauri's webview.

### WebSocket requests

A request file whose `[request]` section declares `protocol: websocket`
(see [the `.nova` file format](./nova-file-format.md)) opens in
`WebSocketPanel.vue` instead of `RequestPanel.vue`. `App.vue`'s tab system
picks between the two per open tab, keyed off `RequestFile.protocol` — a
field populated at collection-discovery time the same cheap, best-effort
way `RequestFile.method` always has been (see
`nova_engine::request::detect_method_and_protocol`), so the sidebar can
show a "WS" badge (the `.method-badge--ws` modifier in
`_sidebar.scss`, alongside the existing per-HTTP-method ones) without a
round trip per request.

The panel edits a `WebSocketDraft` (URL, headers, and the ordered list of
plain-text messages to send once connected) via the
`read_websocket_request`/`save_websocket_request` Tauri commands —
the `nova-engine` counterparts to `read_request`/`save_request`/
`RequestDraft`, going through `ParsedWebSocketRequest::to_nova_string`/
`RequestFile::write_websocket` on the Rust side rather than the GUI
hand-rolling `.nova` syntax. The messages list has no natural fit in
`KeyValueEditor.vue` (a name/value table) and isn't a general-purpose
component of its own — it's a small, purpose-built add/remove/reorder list
inline in `WebSocketPanel.vue`.

Clicking Connect (saving first if the request has unsaved edits, mirroring
`RequestPanel.vue`'s save-before-send behavior) calls the
`connect_websocket` Tauri command, which resolves `{{variable}}`s against
the selected environment (and the request's owning collection's variables)
the same way `send_request` does, then calls
`nova_engine::connect_and_exchange` with `nova_engine::DEFAULT_READ_TIMEOUT`
(5 seconds) and returns a `WebSocketExchange`. Unlike sending an HTTP
request, this doesn't go through the project's persistent `Session` — a
WebSocket connection here is a one-shot connect/send/collect with no
cookies, history, or request-chained variables to participate in. The
panel's transcript shows every sent message followed by everything
received, in two ordered groups rather than a single interleaved timeline:
the engine sends all of a request's declared messages first and only then
reads back whatever comes in, so there's no real interleaving order to
preserve.

### Response diffing

The response pane's Diff tab compares a request's most recent send against
either the send immediately before it in this project's session history
("vs Previous Run") or the request's own hand-written `[response]` example
("vs Saved Example", shown only when the request file has one). The
comparison itself — status/header/body changes, with a structural,
path-addressed diff for JSON bodies and a line-based diff otherwise — is
computed in `nova-engine`'s `diff` module (`diff_responses`), reached via
the `diff_against_previous_run`/`diff_against_example_response` Tauri
commands; the GUI only renders the resulting `ResponseDiff`. Since a
`HistoryEntry` doesn't carry the `.nova` path it came from, "the same
request" is identified by matching method and fully-resolved URL rather
than the source file — see `resolved_identity` in `commands.rs` for the
documented edge cases this trades off.

## The enforced boundary

Neither the GUI nor the CLI ever parses a `.nova`/`nova.yaml`/environment
file, walks the collections directory, or resolves `{{variable}}`s itself —
that always goes through `nova-engine`, so both interfaces stay identical in
behavior. Keep `src/types/nova.ts` and `src/api/nova.ts` in sync with the
Rust types rather than growing frontend-side project logic.

## Styling

Vue single-file components never carry `<style>` blocks. All CSS lives under
`src/styles/` as Sass partials (`_variables.scss`, `_base.scss`,
`components/_*.scss`), assembled by `main.scss` and imported once from
`main.ts`. Components only reference the resulting global class names — do
not regress this by adding a component-local `<style>` block.

## Related

- [The `.nova` request file format](./nova-file-format.md)
- [`nova.yaml` and project structure](./project-structure.md)
