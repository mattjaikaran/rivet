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

## Phase 2 - Context engine (complete 2026-09-09)

Served by `prompts/prompt-05-context.md`. The `.rivet/` store persists
command history, sessions, and fingerprints next to the app module; a
LanceDB vector index answers `rivet explain`. Decisions recorded in
`docs/pillars/04-persistent-context-engine.md`.

### 2.1 Local store

- `.rivet/` layout and SQLite schema (`commands`, `sessions`,
  `fingerprints`) behind `rivet-cli/src/store/`; rusqlite (bundled) chosen
  for portability and recorded in pillar 04; idempotent migration via
  `PRAGMA user_version` (`617632e`).
- `rivet history` lists recorded commands newest-first with exit status
  and duration; invocations are recorded before they run and finished with
  their status and duration (`617632e`, `68e93f9`).
- `rivet session save` renders the current module context (parsed
  blueprint, gauntlet config, diagnostics) as compact markdown and stores
  it; `session resume` prints it back; `session list` names the saved
  sessions (`617632e`).

### 2.2 Semantic search

- LanceDB behind `store::vector`: blueprint routes become deterministic
  character-trigram chunks in `.rivet/lancedb`; an async test indexes two
  routes and ranks the orders route first for an orders symptom
  (`8dd08e7`).
- `rivet explain "<symptom>"` embeds the symptom, finds the nearest route,
  and reports the introducing commit via git pickaxe on the handler; the
  module digest is stored per commit (`8dd08e7`).

### 2.3 Docs

- Pillar 04 rewritten with the store layout, crate choices (rusqlite
  bundled; lancedb 0.38 requiring the `remote` feature), and the commands;
  phase-2 ROADMAP checkboxes ticked; tracker section closed in this commit.

### Verification

- 106 tests green; fmt, clippy (`-D warnings`), and `cargo deny check`
  clean.
- End to end: two builds and a failing build appear in `rivet history`
  with correct statuses; a session round-trips save -> resume; on a
  two-commit fixture `rivet explain "orders failing"` names the commit
  that added the orders route.

## Phase 3 - MCP and agentic CLI

Served by `prompts/prompt-06-mcp.md`. Entries land here as their tracker
lines finish; the section closes when the phase does.

### 1.1 SDK pin

- Pinned `rmcp` 3.2 as the MCP server SDK: it is the official Rust SDK
  for the Model Context Protocol (`modelcontextprotocol/rust-sdk`,
  Apache-2.0), actively released, and compiles at the workspace MSRV (its
  `rust-version` is 1.88, below the workspace 1.91), so no `rust-version`
  bump was needed. Added to the workspace manifest with the `server`,
  `macros`, and `transport-io` features; the choice is recorded in pillar
  09. Tool parameters declare their JSON schema by hand because the
  schemars derive expands to banned `unwrap` calls (`21e2cb6`).
- `rivet mcp` serves the tool router over the stdio transport; an
  in-crate protocol test drives the real server over an in-memory duplex
  through `initialize`, `tools/list`, and `tools/call` and reads valid
  responses. The first tool, `parse_app`, returns the IR blueprint and
  Gauntlet findings for a DSL module as JSON (`21e2cb6`).

### 1.2 MCP tool set

- Completed the `rivet mcp` tool set, each tool a thin wrapper over an
  existing command or store function: `audit_app` (via the new
  `audit_json` seam), `vector_search` and `explain_symptom` (over the
  phase-2 LanceDB index), `session_context`, and `history`. The protocol
  probe test lists all six tools and calls two over the wire; unit tests
  cover every payload (`5642450`).
- Split `rivet explain` into an async core (`explain_async`) plus a sync
  runtime wrapper (`explain_data`) so the MCP server can await the vector
  store without nesting tokio runtimes, and shared `route_summaries`
  between the command and the tools (`5642450`).

### 1.3 Slash commands

- `rivet /plan`, `/fix`, and `/trace` dispatch through clap subcommands;
  main strips a leading `/` from the subcommand token so agents can type
  the slash form exactly. Each command records in the store and returns
  structured diagnostics (`59ffc8a`).
- `/trace "<symptom>"` follows a symptom from the matched DSL route
  (phase-2 vector index) through the axum code `generate_project` would
  render (registration line and handler signature) to the introducing
  commit via git pickaxe. Integration test on a two-commit git fixture
  (`59ffc8a`).
- `/fix` re-runs the Gauntlet and applies only deterministic, safe
  repairs by source span: E2044 dead helpers and DTOs, and E2046 runtime
  classes, foreign functions, and stray statements. It converges in up to
  five parse/re-check rounds and never deletes a route. Integration test
  on a fixture with a dead helper and a runtime class (`59ffc8a`).

### 1.4 Auto-PR generation

- `/plan "<story>"` creates branch `rivet/plan/<slug>`, writes the module
  and a SPEC.md, converges against parse + Gauntlet + a real `rivet
  build`, commits, and prints a PR body with the audit grade; `--push`
  opens the PR through `gh`. Code comes from an OpenAI-compatible
  provider (`RIVET_PLAN_BASE_URL`, `RIVET_PLAN_API_KEY`,
  `RIVET_PLAN_MODEL`) or a prepared module via `--from` — the
  deterministic path the gate exercises. Offline unit tests cover prompt
  assembly and response parsing; the integration test proves a fixture
  story lands on a branch whose crate compiles (`59ffc8a`).

### 1.5 Agentic diagnostics

- `suggested_fix` is now a required `String` on `Diagnostic` and
  `Finding`; every construction site carries a remediation written from
  its error code's meaning, and the JSON payload always emits the field
  (`52f87ed`). Parser error paths, every Gauntlet rule, and the store
  error paths assert non-empty fixes; a broad invariant test in
  `gauntlet/mod.rs` runs a fixture that trips E2043-E2046 and checks
  every output diagnostic (`52f87ed`).

## Repo maintenance

- `scripts/clean.sh` plus `make clean` / `make clean-all` remove the
  generated crates (`examples/*/generated`, about 140 MB each with their
  cargo targets) and the Rivet test fixtures in the temp dir; the old
  `make clean` pointed at a root `generated` path that does not exist
  (`53d70ca`).
- `rivet-cli/src/test_support.rs` adds a `ScratchDir` guard that removes
  each test fixture on drop, on success and on panic, and drops the
  run-pid from fixture names; before this the `/plan` fixture leaked a
  143 MB compiled crate per run (`4c638c7`).

## Phase 4 - Ecosystem and multi-service

Served by `prompts/prompt-07-ecosystem.md`. Entries land here as their
tracker lines finish; the section closes when the phase does.

### Plugin system

- Added the `rivet-plugin-api` workspace crate: the `Plugin` trait plus a
  generic `install` composition primitive, so every call site monomorphizes
  and the built binary carries a direct call instead of a registry lookup.
  `install` logs the plugin name once, at startup (`53242fd`).
- `rivet.toml` grows a `[plugins]` table (`crate`, `path`, `version`), and
  `rivet add plugin` records an entry while keeping every other line of the
  file byte for byte, comments included. A parent table the command creates
  is marked implicit, so a file gains no empty `[plugins]` header; the edit
  validates through the same resolver the build uses and writes nothing when
  it rejects (`E3016` for an unresolvable plugin, `E3017` for a file it
  cannot parse) (`53242fd`).
- `generate_project` resolves every plugin before it writes, adds one
  dependency per plugin to the generated manifest, and emits one install
  call per plugin into `main.rs` in name order — no `dyn`, no name table
  (`53242fd`).
- Shipped the reference plugin `examples/basic/plugins/auth-token` as a
  workspace member: it reads `RIVET_AUTH_TOKEN` once at install time, serves
  `GET /auth/check`, and compares the presented bearer token in constant
  time. The example project composes it (`53242fd`).

### Verification

- 119 CLI tests plus 11 plugin tests and 1 doctest green; fmt, clippy
  (`-D warnings`), and `cargo deny check` clean; the gate passes end to end.
- End to end: `rivet build examples/basic/app.py` writes a manifest that
  depends on `rivet-plugin-auth-token` by path and a `main.rs` with one
  install call; the running binary answers `GET /ping` as before and
  `GET /auth/check` with `authenticated: false` without the header and
  `true` with the configured token (`53242fd`).
