# Testing strategy

| Level | What it covers |
| :--- | :--- |
| Unit tests | IR types, the DSL expression translator, parser rules, and generator output, per module |
| Integration tests | The full pipeline: `rivet build` on `examples/basic/app.py` produces a crate that compiles and answers HTTP requests |
| Mutation tests | `cargo-mutants` (planned with the Gauntlet) ensures tests catch edge cases |
| Performance | `criterion` benchmarks for parse and generation latency (planned) |

## Current gates

The workspace keeps the bar enforced by CI:

- `cargo fmt --all -- --check`
- `cargo clippy -- -D warnings` (the project disallows `unwrap`/`expect` in
  non-test code)
- `cargo test --workspace`

The Gauntlet (phase 1) adds complexity limits, coverage floors, and mutation
survival as compile-time gates on the DSL.
