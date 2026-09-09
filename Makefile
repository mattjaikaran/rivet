.PHONY: help build dev test docker-up docker-down clean

help:
	@echo "Available commands:"
	@echo "  build          - Build the Rivet CLI"
	@echo "  dev            - Run the development environment with Docker"
	@echo "  test           - Run all tests with cargo"
	@echo "  docker-up      - Start dependencies (Postgres + Redis) in Docker"
	@echo "  docker-down    - Stop Docker dependencies"
	@echo "  clean          - Clean all build artifacts"

build:
	cargo build --release --bin rivet

dev:
	@echo "🚀 Starting Rivet in development mode..."
	docker compose --profile dev up -d postgres redis
	@echo "✅ Databases ready. Run 'cargo run --bin rivet dev' to start the CLI."

test:
	cargo test --workspace -- --nocapture

docker-up:
	docker compose --profile dev up -d

docker-down:
	docker compose down

clean:
	cargo clean
	rm -rf ./generated
	rm -rf ./.rivet