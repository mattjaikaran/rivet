# Rivet

**Rivet** is a next-generation API framework: you write your API in a Python
DSL and the Rivet CLI transpiles it into a fast, memory-safe Rust server built
on [axum](https://github.com/tokio-rs/axum).

The project is in early development. The [phase-0
spike](docs/phase-0-spike.md) works end to end; phase 1 (the Gauntlet)
is closed except the mutation tester. `rivet build` parses a Python
module, runs the quality rules between parse and generate, and compiles
a runnable Rust binary; `rivet audit` reports the MQI grade; the
[context engine](docs/pillars/04-persistent-context-engine.md) records
history, sessions, and a blueprint vector index behind `rivet explain`;
and phase 3 ships `rivet mcp` plus the agentic slash commands `/plan`,
`/fix`, and `/trace`. The long-term design lives in
[`docs/`](docs/ARCHITECTURE.md) and the roadmap in
[`docs/ROADMAP.md`](docs/ROADMAP.md).

## Status

Pre-alpha. The [roadmap](docs/ROADMAP.md) tracks the five v1 phases;
phases 0-3 are closed (phase 1 minus the mutation tester, which stays
blocked on the generated-code test story). Phase 4 is in progress: the
compile-time plugin system has landed, so `rivet.toml` can compose plugins
into the generated binary through
[`rivet-plugin-api`](rivet-plugin-api/), with no registry and no runtime
lookup, and `[transport] mode` runs the same blueprint in one process
(a direct call) or over gRPC (the app serves its own channel). `rivet dev`
runs the API and a frontend dev server behind one origin and tunnels the
frontend's HMR socket, and `[frontend] dist` compiles the production
frontend build into the binary, so a shipped app serves its assets from
its own memory. `[admin] enabled = true` compiles a route-table panel into
that binary at `/__rivet/`, and `[discovery]` registers the app with
Consul or etcd and removes it again on shutdown. `rivet mcp` serves the
parser, audit, vector, and context-store tools to AI agents over the Model
Context Protocol, and every JSON diagnostic carries a `suggested_fix`.

## Try it

```bash
# Build the CLI
cargo build --release --bin rivet

# Transpile the example app and compile the generated Rust server
./target/release/rivet build examples/basic/app.py

# Run it
./examples/basic/generated/target/release/basic

# In another shell
curl localhost:3000/ping
# {"status":"pong"}

curl -X POST localhost:3000/echo \
  -H 'Content-Type: application/json' \
  -d '{"hello":"world"}'
# {"echo":{"hello":"world"}}
```

To develop an app with a frontend, start the frontend dev server and run
the proxy beside it. The proxy serves the blueprint's routes and `/api/*`
from the Rust backend, sends every other path to the frontend, and tunnels
the frontend's HMR socket:

```bash
./target/release/rivet dev examples/basic/app.py
# Detected vite: the backend keeps 2 route(s) and /api/*, port Some(5173) serves the rest
# rivet dev listening on http://127.0.0.1:3000
```

For production, point `rivet.toml` at the built frontend and the binary
carries it:

```bash
./target/release/rivet build examples/basic/app.py
# Embedding static assets from examples/basic/dist
# Binary: examples/basic/generated/target/release/basic

./examples/basic/generated/target/release/basic
curl localhost:3000/          # the embedded index.html
```

See [pillar 3](docs/pillars/03-polyglot-frontend-support.md) for the
detection table, the routing rules, and the embedded-asset rules.

The example app is a plain Python file:

```python
from rivet import api

@api.get("/ping", stories=["US-001"])
def ping() -> dict:
    return {"status": "pong"}

@api.post("/echo", stories=["US-002"])
def echo(request: dict) -> dict:
    return {"echo": request}
```

Each `rivet build` writes a crate with its own cargo `target/` (about
140 MB). Remove the generated crates and the test fixtures when you do
not need them:

```bash
make clean        # generated crates and test fixtures
make clean-all    # the above plus the workspace cargo cache
```

## What Rivet transpiles

- `@api.get|post|put|delete|patch|options|head(path, stories=[...])` routes
- handler signatures with type hints: one optional JSON-body parameter
  (`dict`, a DTO, or a primitive) and a return annotation (`dict`,
  primitives, or `-> None`)
- annotation-only DTO classes (`class OrderCreate: sku: str`)
- handler bodies that return literals, request values, or a single DTO
  constructor

Anything outside the subset fails the build with a structured, machine-readable
JSON error instead of a silent mistranslation. Full Python semantics are the
goal of later phases; see [`docs/ROADMAP.md`](docs/ROADMAP.md).

## Repository layout

| Path | Purpose |
| :--- | :--- |
| `constraint-tools/` | Deterministic self-checks that gate the Rivet tree (file length, rule modules, tracker) |
| `rivet-core/` | The IR and shared types for the pipeline |
| `rivet-plugin-api/` | The compile-time plugin contract: the `Plugin` trait and the monomorphized `install` |
| `rivet-cli/` | The `rivet` CLI: parser, generator, build command |
| `examples/basic/plugins/` | The reference plugin (`auth-token`), composed by the example app |
| `examples/basic/` | The smoke-test app used by phase 0 |
| `scripts/` | The `gate.sh` orchestrator running every repo gate |
| `docs/` | Architecture, roadmap, and the nine pillars |
| `prompts/` | The phase-by-phase build prompts that scaffolded this repo |

## License

Licensed under either of [Apache-2.0](LICENSE-APACHE) or
[MIT](LICENSE-MIT) at your option.
