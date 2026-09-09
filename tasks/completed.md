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
