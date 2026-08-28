# nova-app

Nova's desktop application: a Tauri shell (`src-tauri/`) around a Vue 3 +
TypeScript frontend (`src/`), backed entirely by the `nova-engine` crate —
this app never parses project files itself.

Run `make dev` from the repo root to start it in dev mode, or `make
build-app` for a release bundle. See
[`docs/reference/gui.md`](../../docs/reference/gui.md) for the app's layout
and conventions.

## Recommended IDE Setup

- [VS Code](https://code.visualstudio.com/) + [Vue - Official](https://marketplace.visualstudio.com/items?itemName=Vue.volar) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)
