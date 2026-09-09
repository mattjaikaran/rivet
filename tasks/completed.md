# Completed tasks

Tasks are moved here from `tasks/todo.md` when their definition of done is
met. Each entry notes the date and the commit that finished it.

## Phase 0 - The Transpiler Spike (complete 2026-09-09)

Served by `prompts/prompt-00-scaffold.md` through `prompts/prompt-03-generator.md`.
Commits: `dd8920f` (foundation) and `77fe374` (feature).

### Foundation and repo hygiene

- Reset workspace and member manifests to the dependency set each phase needs;
  removed duplicated `rivet-cli` manifest and aspirational pins
  (`dd8920f`).
- Replaced placeholder metadata: repository now `mattjaikaran/rivet`, authors
  set, no invented emails anywhere in docs or security files (`dd8920f`).
- Rewrote Dockerfile (CLI-only multi-stage), docker-compose (Postgres + Redis
  dev profile), and Makefile; removed emoji shell output and the unused
  entrypoint script (`dd8920f`).
- Cleaned `.github` docs (CONTRIBUTING, SECURITY, CODE_OF_CONDUCT) and made CI
  wording honest about planned gates (`dd8920f`).
- Aligned clippy config with the current schema (`too-many-*` kebab-case) and
  removed unstable rustfmt options (`dd8920f`).
- Wrote README, `rivet.toml`, `.dockerignore`, and updated gitignore so nested
  `generated/` output stays out of git (`dd8920f`).
- Rebased docs to reality: ROADMAP phase-0 checkboxes, phase-0 spike status,
  testing-strategy cleanup (`dd8920f`).

### Prompt 01 - IR (rivet-core)

- `rivet-core/src/ir.rs`: `HttpMethod`, `TypeRef`, `FieldDefinition`,
  `StructDefinition`, `RequestSpec`, `ResponseSpec`, `Expr`, `RouteDefinition`,
  `ServiceBlueprint`; serde round-trip tests (`77fe374`).

### Prompt 02 - Python parser (rivet-cli)

- Modular parser under `rivet-cli/src/parser/`: `decorator`, `signature`,
  `body`, `expr`, `types`, `validate`, orchestrated by `python.rs`
  (`77fe374`).
- tree-sitter 0.27 / tree-sitter-python 0.25 based parsing of `@api.<method>`
  decorators, typed signatures, annotation-only DTO classes, and a documented
  handler-body expression subset with string-literal decoding (`77fe374`).
- Structured diagnostics E1001-E1011 emitted as machine-readable JSON
  (`77fe374`).

### Prompt 03 - Rust generator

- `rivet-cli/src/transpiler/rust.rs` renders DTO structs, axum handlers, the
  router, and an isolated `Cargo.toml` (`77fe374`).
- `rivet build` reads `rivet.toml`, writes the crate to `generated/`, and
  compiles it (`77fe374`).

### Verification

- 30 tests green across the workspace; `cargo clippy -- -D warnings` clean;
  `cargo fmt --check` clean.
- Live spike verified from the compiled binary: `GET /ping` returns
  `{"status":"pong"}` and `POST /echo` echoes the JSON body.

## Phase 1 - The Gauntlet (foundation and rules, 2026-09-09)

Served by `prompts/prompt-04-gauntlet.md`. Sections 1.1 and 1.2 are done;
the audit (1.3) and CI (1.4) tasks remain in `tasks/todo.md`.
Commit: `3e09b54`.

### 1.3 The MQI

- Implemented `rivet audit` (grade plus JSON breakdown) per pillar 06:
  complexity, duplicate code, dead code, and type strictness carry the
  pillar weights into an A+ to F grade; a blocker finding deducts 20 points
  and a warning 10. Unit tests cover the aggregation and the grade bands
  (`55e599d`).


- Decided and documented how coverage and mutation survival enter the MQI:
  they stay Rust-side until the generated-code test story exists and appear
  in the `rivet audit` `not_scored` JSON list with their reasons; written
  into pillar 06 and the audit module doc (`bdcb65e`).

### 1.1 Foundation

- Authored `prompts/prompt-04-gauntlet.md` in the prompts 00-03 format;
  every 1.1-1.4 task traces to it (`3e09b54`).
- Defined the Gauntlet rule interface under `rivet-cli/src/gauntlet/`: one
  module per rule, a severity model (blocker/warning), a `Rule` trait, and
  findings that serialize to the agentic-JSON diagnostic shape; unit tests
  cover the harness (`3e09b54`).
- Wired the Gauntlet between parse and generate in `commands/build.rs`;
  blockers return on the `Diagnostic` JSON path with no crate written, and
  warnings print while the build continues (`3e09b54`).

### 1.2 Rules

- Cyclomatic complexity walker over DSL handler bodies and DTO classes,
  failing above `rivet.toml` `[gauntlet] max_complexity`: a synthetic
  handler with score 10 fails with `E2042` and an `ast_path` naming the
  crossing decision; the `examples/basic` handlers pass (`3e09b54`).
- Duplicate-code detector over handler implementation fingerprints: two
  byte-identical handlers produce one `E2043` finding naming both
  locations and the build blocks by severity (`3e09b54`).
- Dead-code rule for helpers and DTOs: an unused helper triggers the
  configured outcome (`E2044`, warning by default, `block` overridable)
  and tests lock the behavior (`3e09b54`).
- Story-to-code gate (pillar 05): a route without `stories=[...]` fails
  with `E2045` and a fix suggestion; the example passes the default gate
  (`3e09b54`).
- Type-strictness rule closing the module-level gaps the parser leaves to
  the IR: runtime classes, foreign decorators, and stray statements fail
  with `E2046`; the accepted dynamic shapes are listed in the rule doc
  (`3e09b54`).

### Verification

- 62 tests green; `cargo clippy -- -D warnings` and `cargo fmt --check`
  clean (`3e09b54`).
- End to end: `rivet build examples/basic/app.py` passes the gate, the
  binary answers `GET /ping` and `POST /echo`, a storyless route exits 1
  with JSON on stderr and no crate written, and a dead-code warning prints
  JSON while the build succeeds (`3e09b54`).
