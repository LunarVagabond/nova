SHELL := /usr/bin/env bash

.PHONY: help install build build-engine build-cli build-app dev run test test-engine \
        test-cli validate fmt fmt-check lint clean stop release-checks release \
        release-skip-tests build-release-bundles stage-release

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

test: test-engine test-cli ## Run all Rust tests

test-engine: ## Run nova-engine's test suite
	cargo test -p nova-engine

test-cli: ## Run nova-cli's test suite
	cargo test -p nova-cli

fmt: ## Format all Rust code
	cargo fmt --all

fmt-check: ## Check Rust formatting without modifying files
	cargo fmt --all -- --check

lint: ## Run clippy across the workspace
	cargo clippy --workspace --all-targets -- -D warnings

clean: ## Remove build artifacts (Rust target dir and frontend build output)
	cargo clean
	rm -rf $(APP_DIR)/dist $(APP_DIR)/node_modules

release-checks: ## Run the pre-release validation gate (fmt-check, lint, test, frontend build) standalone
	bash scripts/release-checks.sh

build-release-bundles: ## Build all three release bundle combos (Engine+CLI, Engine+GUI, Engine+CLI+GUI)
	bash scripts/build-release-bundles.sh

stage-release: ## Copy/rename built bundle artifacts into release-files/ (use VER=X.Y.Z, OUT=dir)
	bash scripts/stage-release.sh $(if $(VER),$(VER),$(error VER is required, e.g. make stage-release VER=0.2.0)) $(OUT)

.PHONY: release
## Run pre-release checks, then update version files, commit, and create a release tag.
##
## Usage:
##   make release VER=0.2.0 TITLE="Some release title"
## - VER prompts if missing (shows current version). TITLE is optional.
## - Tag format:
##   - If TITLE is provided: v<VER>-<TITLE_SLUG>
##   - If TITLE is empty:    v<VER>
## - Runs scripts/release-checks.sh first; aborts with no changes made if it fails.
## - Bumps Cargo.toml, nova-app/package.json, and nova-app/src-tauri/tauri.conf.json.
release:
	@set -euo pipefail; \
	if [ "$(SKIP_CHECKS)" = "1" ]; then \
		echo "release: skipping pre-release checks (SKIP_CHECKS=1) -- only use this if you already ran and passed them"; \
	else \
		echo "release: running pre-release checks (scripts/release-checks.sh)"; \
		if ! bash scripts/release-checks.sh; then \
			echo "release: pre-release checks failed -- nothing was changed, fix and re-run 'make release'"; \
			exit 1; \
		fi; \
	fi; \
	ver="$(strip $(VER))"; \
	current_ver="$$(sed -n '/^\[workspace\.package\]/,/^\[/p' Cargo.toml | sed -nE 's/^version[[:space:]]*=[[:space:]]*"([^"]+)".*/\1/p' | head -n1)"; \
	if [ -z "$$ver" ]; then \
		if [ -n "$$current_ver" ]; then \
			read -r -p "Release version (current: $$current_ver): " ver; \
			if [ -z "$$ver" ]; then ver="$$current_ver"; fi; \
		else \
			read -r -p "Release version (X.Y.Z): " ver; \
		fi; \
	fi; \
	if ! [[ "$$ver" =~ ^[0-9]+\.[0-9]+\.[0-9]+$$ ]]; then \
		echo "release: invalid VER '$$ver' (expected X.Y.Z)"; \
		exit 1; \
	fi; \
	title="$(strip $(TITLE))"; \
	if [ -z "$$title" ]; then \
		read -r -p "Release title (optional): " title; \
	fi; \
	title_slug="$$(printf '%s' "$$title" | sed -E 's/[[:space:]]+/-/g; s/[^A-Za-z0-9._-]//g; s/^-+//; s/-+$$//')"; \
	tag_name="v$$ver"; \
	if [ -n "$$title_slug" ]; then \
		tag_name="$$tag_name-$$title_slug"; \
	fi; \
	echo "release: tag='$$tag_name' version='$$ver'"; \
	if git rev-parse -q --verify "refs/tags/$$tag_name" >/dev/null; then \
		echo "release: git tag '$$tag_name' already exists"; \
		exit 1; \
	fi; \
	tmp_file="$$(mktemp)"; \
	awk -v ver="$$ver" '\
	BEGIN { in_section=0 } \
	$$0 == "[workspace.package]" { in_section=1; print; next } \
	in_section && $$0 ~ /^\[/ { in_section=0 } \
	in_section && $$0 ~ /^version[[:space:]]*=/ { $$0 = "version = \"" ver "\"" } \
	{ print }' Cargo.toml > "$$tmp_file"; \
	mv "$$tmp_file" Cargo.toml; \
	node -e 'const fs=require("fs"); const p="$(APP_DIR)/package.json"; const j=JSON.parse(fs.readFileSync(p,"utf8")); j.version=process.argv[1]; fs.writeFileSync(p, JSON.stringify(j,null,2)+"\n");' "$$ver"; \
	node -e 'const fs=require("fs"); const p="$(APP_DIR)/src-tauri/tauri.conf.json"; const j=JSON.parse(fs.readFileSync(p,"utf8")); j.version=process.argv[1]; fs.writeFileSync(p, JSON.stringify(j,null,2)+"\n");' "$$ver"; \
	cargo check -q --workspace || true; \
	version_files="Cargo.toml Cargo.lock $(APP_DIR)/package.json $(APP_DIR)/package-lock.json $(APP_DIR)/src-tauri/tauri.conf.json"; \
	git add -- $$version_files; \
	if git diff --cached --quiet -- $$version_files; then \
		echo "release: no version changes to commit (continuing with tag)"; \
	else \
		git commit -m "Release $$tag_name"; \
	fi; \
	git tag -a "$$tag_name" -m "$$tag_name"; \
	echo "release done: $$tag_name"; \
	echo "Next: git push origin main --tags"

.PHONY: release-skip-tests
## Same as 'release', but skips scripts/release-checks.sh.
## Only use this when you already ran the checks (or are otherwise highly confident they pass) and don't want to re-run/wait on them.
release-skip-tests: SKIP_CHECKS=1
release-skip-tests: release
