.PHONY: help build dev test docker-up docker-down clean

help: ## Show available commands
	@grep -E '^[a-zA-Z_-]+:.*?## ' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  %-12s %s\n", $$1, $$2}'

build: ## Build the Rivet CLI
	cargo build --release --bin rivet

dev: ## Start local dependencies (Postgres + Redis) in Docker
	docker compose --profile dev up -d

test: ## Run all workspace tests
	cargo test --workspace

docker-up: ## Start Docker dependencies
	docker compose --profile dev up -d

docker-down: ## Stop Docker dependencies
	docker compose down

clean: ## Clean build artifacts and generated output
	cargo clean
	rm -rf generated
