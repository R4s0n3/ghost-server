SHELL := /bin/bash
.DEFAULT_GOAL := help

PORT ?= 9001
BASE_URL ?= http://localhost:$(PORT)
PDF ?= /absolute/path/sample.pdf
CLERK_JWT ?=
API_KEY ?=
STRIPE_FORWARD_URL ?= localhost:$(PORT)/api/stripe/webhook

.PHONY: help run dev convex check fmt clippy build clean \
	health preflight-test smoke \
	gen-api-key list-api-keys api-analyze api-grayscale \
	stripe-listen

help: ## Show available commands
	@grep -E '^[a-zA-Z0-9_.-]+:.*## ' Makefile | sed -E 's/:.*## /\t/'

run: ## Run Rust API server (loads .env automatically via dotenvy)
	cargo run --bin ghost-api-server

dev: run ## Alias for run

convex: ## Run local Convex dev server
	bunx convex dev

check: ## Type-check/build the Rust server
	cargo check

fmt: ## Format Rust code
	cargo fmt --all

clippy: ## Run clippy lints
	cargo clippy --all-targets --all-features

build: ## Build release binary
	cargo build --release --bin ghost-api-server

clean: ## Remove build artifacts
	cargo clean

health: ## Call health endpoint
	curl -i $(BASE_URL)/health/

preflight-test: ## Test public PDF preflight endpoint (set PDF=/path/file.pdf)
	@test -f "$(PDF)" || { echo "PDF not found: $(PDF)"; exit 1; }
	curl -i -F "file=@$(PDF)" $(BASE_URL)/process/preflight-test

smoke: health preflight-test ## Run basic smoke test

gen-api-key: ## Create API key (set CLERK_JWT=<token>)
	@test -n "$(CLERK_JWT)" || { echo "CLERK_JWT is required"; exit 1; }
	curl -i -X POST $(BASE_URL)/api/keys/ \
		-H "Authorization: Bearer $(CLERK_JWT)"

list-api-keys: ## List API keys (set CLERK_JWT=<token>)
	@test -n "$(CLERK_JWT)" || { echo "CLERK_JWT is required"; exit 1; }
	curl -i $(BASE_URL)/api/keys/ \
		-H "Authorization: Bearer $(CLERK_JWT)"

api-analyze: ## Analyze PDF via API key route (set API_KEY and PDF)
	@test -n "$(API_KEY)" || { echo "API_KEY is required"; exit 1; }
	@test -f "$(PDF)" || { echo "PDF not found: $(PDF)"; exit 1; }
	curl -i -F "file=@$(PDF)" \
		-H "X-API-Key: $(API_KEY)" \
		$(BASE_URL)/api/process/analyze

api-grayscale: ## Convert PDF to grayscale via API key route (set API_KEY and PDF)
	@test -n "$(API_KEY)" || { echo "API_KEY is required"; exit 1; }
	@test -f "$(PDF)" || { echo "PDF not found: $(PDF)"; exit 1; }
	curl -i -F "file=@$(PDF)" \
		-H "X-API-Key: $(API_KEY)" \
		$(BASE_URL)/api/process/grayscale \
		-o grayscale-output.pdf

stripe-listen: ## Forward Stripe test webhooks to local server
	stripe listen --forward-to $(STRIPE_FORWARD_URL)
