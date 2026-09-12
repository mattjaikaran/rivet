.PHONY: help build dev test docker-up docker-down clean clean-dry-run clean-all disk gate self-check

help: ## Show available commands
	@grep -E '^[a-zA-Z_-]+:.*?## ' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  %-16s %s\n", $$1, $$2}'

build: ## Build the Rivet CLI
	cargo build --release --bin rivet

dev: ## Start local dependencies (Postgres + Redis) in Docker
	docker compose --profile dev up -d

test: ## Run all workspace tests
	cargo test --workspace

gate: ## Run every repo gate (fmt, clippy, tests, deny, example, self-checks)
	./scripts/gate.sh

self-check: ## Run the repository self-checks (constraint tools)
	cargo build -p constraint-tools
	./target/debug/check-file-length
	./target/debug/check-rule-modules
	./target/debug/check-tracker

docker-up: ## Start Docker dependencies
	docker compose --profile dev up -d

docker-down: ## Stop Docker dependencies
	docker compose down

clean: ## Remove generated crates and test fixtures (keeps the build cache)
	./scripts/clean.sh

clean-dry-run: ## Show what `clean` and `clean-all` would remove
	./scripts/clean.sh --dry-run

clean-all: ## Remove generated output and the workspace build cache
	./scripts/clean.sh --all

disk: ## Report the disk footprint of generated output
	./scripts/clean.sh --disk
