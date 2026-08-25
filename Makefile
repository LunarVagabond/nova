.PHONY: help install build build-engine build-cli build-app dev run test test-engine \
        validate fmt fmt-check lint clean stop

APP_DIR := crates/nova-app
FIXTURE := crates/nova-engine/tests/fixtures/basic-project

help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*## .*$$' $(MAKEFILE_LIST) | sort | \
		awk 'BEGIN {FS = ":.*## "}; {printf "  \033[36m%-16s\033[0m %s\n", $$1, $$2}'

install: ## Install frontend dependencies for the desktop app
	cd $(APP_DIR) && npm install

build: build-engine build-cli ## Build the engine and CLI (debug)

build-engine: ## Build nova-engine only
	cargo build -p nova-engine

build-cli: ## Build nova-cli only
	cargo build -p nova-cli

build-app: install ## Build the desktop app (release bundle)
	cd $(APP_DIR) && npm run tauri build

dev: install ## Run the desktop app in dev mode (hot reload)
	cd $(APP_DIR) && npm run tauri dev

stop: ## Stop a running dev instance of the desktop app
	pkill -f "$(APP_DIR)/src-tauri/target" 2>/dev/null || true

run: build-cli ## Run the CLI against the bundled example fixture project (use ARGS="..." for other args/paths)
	cargo run -q -p nova-cli -- $(if $(ARGS),$(ARGS),inspect $(FIXTURE))

validate: build-cli ## Validate the example fixture project (use ARGS="path/to/project" for another project)
	cargo run -q -p nova-cli -- validate $(if $(ARGS),$(ARGS),$(FIXTURE))

test: test-engine ## Run all Rust tests

test-engine: ## Run nova-engine's test suite
	cargo test -p nova-engine

fmt: ## Format all Rust code
	cargo fmt --all

fmt-check: ## Check Rust formatting without modifying files
	cargo fmt --all -- --check

lint: ## Run clippy across the workspace
	cargo clippy --workspace --all-targets -- -D warnings

clean: ## Remove build artifacts (Rust target dir and frontend build output)
	cargo clean
	rm -rf $(APP_DIR)/dist $(APP_DIR)/node_modules
