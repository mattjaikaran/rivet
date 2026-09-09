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

## Phase 1 - The Gauntlet (complete 2026-09-09)

Served by `prompts/prompt-04-gauntlet.md`. All four sections are done;
see the per-section entries below. The mutation-tester roadmap bullet
stays open (blocked on the generated-code test story).

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

### 1.3 The MQI

- Implemented `rivet audit` (grade plus JSON breakdown) per pillar 06:
  complexity, duplicate code, dead code, and type strictness carry the
  pillar weights into an A+ to F grade; a blocker finding deducts 20
  points and a warning 10. Unit tests cover the aggregation and the
  grade bands (`55e599d`).
- Decided and documented how coverage and mutation survival enter the
  MQI: they stay Rust-side until the generated-code test story exists
  and appear in the `rivet audit` `not_scored` JSON list with their
  reasons; written into pillar 06 and the audit module doc (`b0c96c8`).

### 1.4 Integration and docs

- The CI `gauntlet-check` job runs the Gauntlet on `examples/basic`: a
  real `rivet build` plus `rivet audit --json`, replacing the
  placeholder `--version` step (`ce70633`).
- Added cargo-deny CVE scanning: `deny.toml` and a CI job that runs
  `cargo deny check`; verified locally that the check fails on
  `RUSTSEC-2020-0071` (`ce70633`).
- Ticked the phase-1 ROADMAP checkboxes, documented the real commands
  in `docs/development-workflow.md`, and aligned `CONTRIBUTING.md`,
  pillar 07, and the README with `rivet audit` (`99f086e`).
- Authored `prompts/prompt-05-context.md` for phase 2 in the prompts
  00-04 format (`24be798`).
- Closed phase 1 in the tracker and roadmap; the mutation-tester
  roadmap bullet stays open, blocked on the generated-code test story
  (see pillar 06, `not_scored`).

## Constraint tools (complete 2026-09-09)

Not a roadmap phase: deterministic self-checks gate the Rivet source tree
the way the Gauntlet gates DSL apps, preceding phase 2 so later work
inherits them. Served by the SwarmForge constraint-tools pattern in
`~/dev/django-ninja-boilerplate/docs/CONSTRAINT_TOOLS.md`.

- Added the `constraint-tools` workspace crate with three small
  deterministic binaries: `check-file-length` (400-line default ceiling,
  300 for Gauntlet rule modules, four grandfathered legacy files frozen
  at 728/699/679/444), `check-rule-modules` (error-code table, `pub mod`
  declarations, rule files, and doc codes agree 1:1), and `check-tracker`
  (no `- [x]` left in todo.md, no duplicate or cross-file task text, no
  open item under a `(complete ...)` section); each exits 0 or 1
  (`cb3e3bf`).
- One gate command: `scripts/gate.sh` runs fmt, clippy (`-D warnings`),
  tests, `cargo deny check`, the example build and audit, and the three
  self-checks in order, stopping at the first failure with its output
  visible; Makefile targets `gate` and `self-check` delegate to it
  (`86d8803`).
- CI wiring: the `repo-self-checks` job builds and runs the three
  self-check binaries on every push to main and PR, so a planted
  violation fails the job with the violation in the log (`4162a09`).
- Self-enforcement and docs: the tools gate themselves (the crate is
  inside the file-length and rule-module checks and passes), and the real
  commands, thresholds, and how-to-add-a-check steps are recorded in
  `docs/development-workflow.md`, `CONTRIBUTING.md`, and the README
  layout table (`25ed04e`).

### Verification

- 93 tests green (20 new), fmt and clippy clean, gate passes end to end.
- A planted over-long rule module (`complexity.rs` at 309 lines) fails
  `check-file-length`, an undocumented rule module fails
  `check-rule-modules`, a leftover `- [x]` line fails `check-tracker`,
  and the full gate exits 1 with the violation visible.
