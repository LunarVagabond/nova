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
- **`src/types/nova.ts`** — hand-mirrors the engine's `Serialize` shapes.
- **`src/api/nova.ts`** — wraps `invoke()` and the dialog plugin.
- **`src/components/`** — `Sidebar.vue`/`ProjectPanel.vue`/`CollectionNode.vue`
  for the collection tree; `RequestPanel.vue` for the request editor, split
  into Params/Auth/Headers/Body tabs (`AuthEditor.vue`, `KeyValueEditor.vue`,
  `MultipartEditor.vue`, `CodeEditor.vue`); `EnvironmentPanel.vue` for
  environment editing; `Modal.vue` is the shared in-app dialog component —
  used instead of `window.prompt`/`window.confirm`, which are unreliable
  inside Tauri's webview.

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
