.DEFAULT_GOAL := help

CARGO ?= cargo
PYTHON ?= python3
NODE ?= node
DEBUG_BIN := target/debug/saumux
RELEASE_BIN := target/release/saumux

.PHONY: help fmt clippy test check build release run pty release-tests browser-deps browser verify clean

help: ## Show available development commands
	@awk 'BEGIN {FS = ":.*##"} /^[a-zA-Z0-9_-]+:.*##/ {printf "  %-16s %s\n", $$1, $$2}' $(MAKEFILE_LIST)

fmt: ## Check Rust formatting
	$(CARGO) fmt --check

clippy: ## Run strict Clippy checks for all targets
	$(CARGO) clippy --locked --all-targets -- -D warnings

test: ## Run Rust unit and integration tests
	$(CARGO) test --locked

check: fmt clippy test ## Run formatting, lint and Rust tests

build: ## Build the debug executable
	$(CARGO) build --locked

release: ## Build the optimized executable
	$(CARGO) build --release --locked

run: build ## Build and start saumux
	$(DEBUG_BIN) $(ARGS)

pty: build ## Check client terminal lifecycle and restoration
	$(PYTHON) tests/client_pty.py $(DEBUG_BIN)

release-tests: ## Run release packaging helper tests
	$(PYTHON) -m unittest discover -s tests -p 'release_*.py'

browser-deps: ## Install the optional Playwright browser test dependencies
	npm ci --prefix tests/browser --ignore-scripts
	npm exec --prefix tests/browser -- playwright install chromium

browser: build ## Run the optional real Chromium browser smoke test
	$(NODE) tests/browser/smoke.cjs $(DEBUG_BIN)

verify: check release pty release-tests ## Run the local checks used by the release build

clean: ## Remove Cargo build artifacts
	$(CARGO) clean
