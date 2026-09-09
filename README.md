# Rivet

**Rivet** is a next-generation API framework: you write your API in a Python
DSL and the Rivet CLI transpiles it into a fast, memory-safe Rust server built
on [axum](https://github.com/tokio-rs/axum).

The project is in early development. The phase-0 spike is working end to
end, and phase 1 (the Gauntlet) is closed except the mutation tester:
`rivet build` parses a Python module, runs the quality rules between parse
and generate, and compiles a runnable Rust binary; `rivet audit` reports
the MQI grade. The long-term design lives in
[`docs/`](docs/ARCHITECTURE.md) and the roadmap in
[`docs/ROADMAP.md`](docs/ROADMAP.md).

## Status

Pre-alpha. One milestone is complete: the [phase-0
spike](docs/phase-0-spike.md) — a working transpilation pipeline for a
documented subset of the DSL. Phase 1 (the [Gauntlet](docs/pillars/07-the-gauntlet.md))
adds compile-time quality rules and the [MQI
audit](docs/pillars/06-matt-quality-index.md) on top.

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

## What phase 0 transpiles

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
| `rivet-core/` | The IR and shared types for the pipeline |
| `rivet-cli/` | The `rivet` CLI: parser, generator, build command |
| `examples/basic/` | The smoke-test app used by phase 0 |
| `docs/` | Architecture, roadmap, and the nine pillars |
| `prompts/` | The phase-by-phase build prompts that scaffolded this repo |

## License

Licensed under either of [Apache-2.0](LICENSE-APACHE) or
[MIT](LICENSE-MIT) at your option.
