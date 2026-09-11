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
that binary at `/__rivet/`, `[discovery]` registers the app with Consul or
etcd and removes it again on shutdown, and `rivet sync` reconciles the
blueprint's story IDs with Jira or Linear. `rivet mcp` serves the parser,
audit, vector, and context-store tools to AI agents over the Model Context
Protocol, and every JSON diagnostic carries a `suggested_fix`.

Phase 5 starts: `rivet build --target wasm` compiles the same blueprint to a
`wasm32-wasip1` module for an edge host, reusing the native target's DTOs and
service layer, so a route's logic has one implementation. Two
`[rust_native_features]` flags have landed: `const_generics` renders
`List[T, N]` as a Rust array, and `zero_copy_deserialization` renders a
`borrowed[str]` DTO field as a `&str` slice of the request body.

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

curl localhost:3000/orders/42
# {"id":42,"status":"open"}

curl -X POST localhost:3000/echo \
  -H 'Content-Type: application/json' \
  -d '{"hello":"world"}'
# {"echo":{"hello":"world"}}

curl 'localhost:3000/search?page=2&size=10'
# {"page":2,"size":10}
```

To develop an app with a frontend, start the frontend dev server and run
the proxy beside it. The proxy serves the blueprint's routes and `/api/*`
from the Rust backend, sends every other path to the frontend, and tunnels
the frontend's HMR socket:

```bash
./target/release/rivet dev examples/basic/app.py
# Detected vite: the backend keeps 5 route(s) and /api/*, port Some(5173) serves the rest
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

For an edge host, build the same blueprint as a WASI module and run it with
Wasmtime:

```bash
rustup target add wasm32-wasip1
./target/release/rivet build --target wasm examples/basic/app.py

echo '{"method":"GET","path":"/ping"}' \
  | wasmtime run examples/basic/generated-wasm/target/wasm32-wasip1/release/basic.wasm
# {"body":{"status":"pong"},"status":200}
```

The module is a per-request edge handler: one JSON request in, one JSON
envelope out. See
[pillar 8](docs/pillars/08-wasm-mobile-sdk-support.md) for the protocol and
what the target omits.

The example app is a plain Python file:

```python
from rivet import api

@api.get("/ping", stories=["US-001"])
def ping() -> dict:
    return {"status": "pong"}

@api.post("/echo", stories=["US-002"])
def echo(request: dict) -> dict:
    return {"echo": request}

@api.get("/orders/{id}", stories=["US-005"])
def get_order(id: int) -> dict:
    return {"id": id, "status": "open"}

@api.get("/search", stories=["US-006"])
def search(page: int, size: int) -> dict:
    return {"page": page, "size": size}
```

`get_order` reads `{id}` from the path, and `search` reads its two values
from the query string. A query string carries text, so only a primitive
reads from it: declare a `dict`, a DTO, or a list and the value is the JSON
body instead, which a handler takes at most once.

A DTO field can borrow its text from the request body instead of copying it:

```python
class Note:
    text: borrowed[str]

@api.post("/notes", stories=["US-004"])
def create_note(request: Note) -> dict:
    return {"echo": request}
```

Set `zero_copy_deserialization = true` in `[rust_native_features]` and the
generated field is `pub text: &'a str` behind `#[serde(borrow)]`. See
[pillar 8](docs/pillars/08-wasm-mobile-sdk-support.md) for the borrow rules
and the diagnostics that hold them.

Each `rivet build` writes a crate with its own cargo `target/` (about
140 MB). Remove the generated crates and the test fixtures when you do
not need them:

```bash
make clean        # generated crates and test fixtures
make clean-all    # the above plus the workspace cargo cache
```

## What Rivet transpiles

- `@api.get|post|put|delete|patch|options|head(path, stories=[...])` routes
- `{name}` path parameters, for example `@api.get("/orders/{id}")` with
  `def get_order(id: int)`; a path parameter takes `str` or `int`
- query parameters, for example `def search(page: int, size: str)`: a
  parameter the route path does not name reads from the query string when
  its type is a primitive (`str`, `int`, `float`, or `bool`)
- at most one request body per handler: a `dict`, a DTO, or a list, plus a
  return annotation (`dict`, primitives, or `-> None`)
- annotation-only DTO classes (`class OrderCreate: sku: str`)
- handler bodies that return literals, request values, or a single DTO
  constructor
- `borrowed[str]` DTO fields (a slice of the request body) under the
  `zero_copy_deserialization` flag, and `List[T, N]` fixed-size arrays under
  `const_generics`

Anything outside the subset fails the build with a structured, machine-readable
JSON error instead of a silent mistranslation. Full Python semantics are the
goal of later phases; see [`docs/ROADMAP.md`](docs/ROADMAP.md).

## Repository layout

| Path | Purpose |
| :--- | :--- |
| `constraint-tools/` | Deterministic self-checks that gate the Rivet tree (file length, rule modules, tracker) |
| `rivet-core/` | The IR, the shared pipeline types, and the reserved names the generated crate owns |
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
