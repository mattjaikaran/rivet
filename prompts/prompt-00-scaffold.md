# Prompt 00: Project Scaffolding & Dockerization

**Objective**: Initialize the Rust workspace and configure the development environment with Docker/Orbstack.

**Context**: We are building a Rust CLI tool called "Rivet" that transpiles Python/TypeScript DSL to Rust.

---

## Tasks

### 1. Cargo Workspace Setup

- Create a workspace with two members:
  - `rivet-core` (library for IR, data structures)
  - `rivet-cli` (binary for the CLI)
- Add these dependencies to `rivet-cli/Cargo.toml`:
  ```toml
  [dependencies]
  rivet-core = { path = "../rivet-core" }
  tokio = { version = "1.40", features = ["full"] }
  axum = "0.7"
  serde = { version = "1.0", features = ["derive"] }
  serde_json = "1.0"
  tree-sitter = "0.22"
  tree-sitter-python = "0.20"
  clap = { version = "4.5", features = ["derive", "env"] }
  anyhow = "1.0"
  tracing = "0.1"
  tracing-subscriber = { version = "0.3", features = ["env-filter"] }
  ```

Add these to rivet-core/Cargo.toml:

```toml
[dependencies]
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
anyhow = "1.0"
```


2. Dockerfile (Multi-Stage)
Create a Dockerfile with the following stages:

- Builder (rust:1.81-alpine):
  - Install build dependencies (musl-dev, pkgconfig, openssl-dev, libpq-dev, build-base).
  - Copy Cargo.toml and Cargo.lock first to cache dependencies.
  - Create dummy src/main.rs to pre-build dependencies.
  - Copy actual source code and build rivet binary.
- Runner (alpine:3.20):
  - Install runtime dependencies (ca-certificates, openssl, libpq, curl).
  - Copy the binary from the builder.
  - Copy docker-entrypoint.sh and set it as the entrypoint.

3. docker-compose.yml
Create a `docker-compose.yml` with:

- PostgreSQL 18 (with pgvector extension):

```yaml
image: postgres:18-alpine
environment:
  POSTGRES_USER: rivet
  POSTGRES_PASSWORD: rivet
  POSTGRES_DB: rivet_dev
ports:
  - "5432:5432"
volumes:
  - pg_data:/var/lib/postgresql/data
command: postgres -c shared_preload_libraries=pgvector
profiles: [dev, full]
```

Redis 7:

```yaml
image: redis:7-alpine
ports:
  - "6379:6379"
volumes:
  - redis_data:/data
profiles: [dev, full]
```
App (optional, for `full` profile):

```yaml
build:
  context: .
  dockerfile: Dockerfile
volumes:
  - ./:/app
ports:
  - "3000:3000"
environment:
  DATABASE_URL: postgres://rivet:rivet@postgres:5432/rivet_dev
  REDIS_URL: redis://redis:6379
depends_on: [postgres, redis]
profiles: [full]
```

4. Makefile
Create a `Makefile` with:

- `make help` - shows available commands
- `make build` - `cargo build --release --bin rivet`
- `make dev` - `docker compose --profile dev up -d`
- `make test` - `cargo test --workspace`
- `make clean` - removes `target/`, `generated/`, and `.rivet/`

5. CLI Skeleton
In rivet-cli/src/main.rs:

- Use `clap` to define a `Command` struct with a `--version` flag.
- Implement `main()` that prints `"Rivet v0.1.0"`.
- Add a subcommand `build` (placeholder that prints `"Building..."`).


## Agent Instructions:
1. Run `cargo new rivet-core --lib` and `cargo new rivet-cli --bin` in the root.
2. Edit the root `Cargo.toml` to define the workspace:
```toml
[workspace]
members = ["rivet-core", "rivet-cli"]
resolver = "2"
```
3. Copy the dependency lists above into the respective `Cargo.toml` files.
4. Create `Dockerfile` with the multi-stage build as specified.
5. Create `docker-entrypoint.sh` with:
```bash
#!/bin/sh
if [ -f "/app/app.py" ]; then
    echo "📦 Building Rivet app..."
    rivet build --release
fi
if [ -f "/app/generated/target/release/app" ]; then
    exec /app/generated/target/release/app "$@"
else
    exec rivet "$@"
fi
```
Make it executable: chmod +x docker-entrypoint.sh.

6. Create `docker-compose.yml` with the services above.
7. Create `Makefile` with the commands above.
8. In `rivet-cli/src/main.rs`, write the CLI skeleton.
