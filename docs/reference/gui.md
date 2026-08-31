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
  `KeyValueEditor.vue`, `MultipartEditor.vue`, `CodeEditor.vue`), plus a
  "Variables" toggle in its header that opens a read-only drawer listing
  what the request's `{{variable}}` placeholders would actually resolve to
  for the selected environment — collection variables and this project's
  session-chained variables included, via the `get_resolved_variables`
  command (`nova_engine::Session::resolved_variables`, the same
  collection/chained/environment merge `send_request` uses). Editing still
  only happens in `EnvironmentPanel.vue`;
  `WebSocketPanel.vue` for a WebSocket request (`protocol: websocket`) — a
  separate component from `RequestPanel.vue` rather than another tab on it,
  since a WebSocket request has no method/params/body/auth/assertions/
  example response to begin with (see below); `EnvironmentPanel.vue` for
  environment editing; `HistoryPanel.vue` for the current project's recent
  sends (method/status/timing/timestamp), reachable from the top bar's
  clock-icon action, with a click on an entry reopening its full stored
  request/response — in-memory and per-session, so it resets when the app
  restarts; `CookiesPanel.vue` for the current project's session cookie jar,
  reachable from the top bar's cookie-icon action — lists every cookie
  currently stored (host/name/value/path/domain/secure/expiry), with inline
  editing of a cookie's value, deleting a single cookie, or clearing the
  whole jar; also in-memory and per-session, resetting along with
  `HistoryPanel.vue`'s history when the app restarts;
  `ResponseDiffView.vue` renders a structured response diff (see
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

The panel edits a `WebSocketDraft` (URL, headers, and the request's saved
messages — see below) via the `read_websocket_request`/
`save_websocket_request` Tauri commands — the `nova-engine` counterparts to
`read_request`/`save_request`/`RequestDraft`, going through
`ParsedWebSocketRequest::to_nova_string`/`RequestFile::write_websocket` on
the Rust side rather than the GUI hand-rolling `.nova` syntax.

**Composer and saved messages.** The top half of the panel is a message
composer: a `CodeEditor` for the message about to be sent, a format
selector (JSON/Text/Binary/XML/HTML — "Binary" is edited as plain text with
beautify disabled, since the engine only ever sends text frames; see
`crates/nova-engine/src/websocket.rs`'s module doc comment) with a beautify
button next to it (reusing the same `beautifyJson`/`formatXml` helpers
`RequestPanel.vue`'s raw body editor uses), and a Send button enabled only
while a session is open. Alongside it, a saved-messages list is just a
picker over the request's `[messages]` list — the on-disk format is
unchanged from the original batch design; there's no new section and no
per-message name field. `nova ws` (the CLI) still sends every message in
`[messages]` in order on connect, exactly as before. Since a saved message
has no name field to show, the side panel labels each entry with a
truncated preview of its own text (mirroring how Postman itself falls back
to a content-derived name) rather than inventing an on-disk naming syntax —
adding a `name:` prefix line was considered and rejected: distinguishing it
unambiguously from a JSON/text message that happens to start the same way
isn't clean, and a fallback preview needs no format or parser change at
all. Clicking a saved entry loads it into the composer for editing/resend;
saving updates that same entry in place, or appends a new one if the
composer wasn't loaded from an existing entry.

**Live session.** Connect (saving first if the request has unsaved edits,
mirroring `RequestPanel.vue`'s save-before-send behavior) calls the
`connect_websocket_session` Tauri command, which resolves `{{variable}}`s
against the selected environment (and the request's owning collection's
variables) the same way `send_request`/the older `connect_websocket` do,
then opens an `nova_engine::WebSocketSession` and keeps it open — unlike
the one-shot `connect_websocket`/`connect_and_exchange` batch flow `nova
ws` (the CLI) still uses, this stays connected so messages can be sent one
at a time via `send_websocket_session_message` and watched as they arrive.
Only one interactive session is open at a time app-wide (managed by
`WebSocketSessionState` in `nova-app/src-tauri`, mirroring
`MockServerState`'s shape); opening Connect on a second WebSocket tab while
one is already live surfaces the rejection as a connect error rather than
silently stealing the connection. Each received message arrives as a
`"ws-session:message"` Tauri event (`listen()`'d from the frontend, not
polled) and an unexpected close (the server hung up) as
`"ws-session:closed"`. The transcript is a single interleaved,
chronologically-ordered list — a sent row is appended synchronously the
moment `send_websocket_session_message` resolves (it doesn't wait on any
reply to know the send happened), a received row is appended whenever the
message event fires — each row showing a direction arrow (↑ sent, ↓
received), the message text, and a timestamp. Disconnect closes the
session and stops listening for its events; closing the tab that opened a
still-live session disconnects it too, so a stray connection doesn't
outlive the tab that owns it.

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
