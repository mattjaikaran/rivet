# Rivet task tracker

This file is the source of truth for what needs to be done. It follows the
[roadmap](docs/ROADMAP.md) phases and the original `prompts/` build order, and
each phase references the pillars it serves.

Last updated: 2026-09-09

## How to use this file

- Keep `tasks/todo.md` authoritative for outstanding work.
- When a task is finished: tick it, add the date and commit, then move the
  line to `tasks/completed.md` in the same change.
- Group work by roadmap phase. Never jump ahead of the phase the session is
  in unless the task says otherwise.
- Every phase starts by authoring its `prompts/prompt-NN-*.md` seed so future
  sessions inherit the same structure the first prompts gave this repo.
- Checkboxes: `- [ ]` open, `- [x]` done (awaiting move), `- [~]` in progress.

## Definition of done

A task is done when all of the following hold:

- `cargo fmt --all -- --check` passes
- `cargo clippy -- -D warnings` passes
- `cargo test --workspace` passes
- Behavior that changed is verified end to end (the `examples/basic` build
  plus live `curl` when the pipeline is touched)
- Docs that the change affects are updated (pillar docs, ROADMAP checkboxes,
  README status)
- The task line is moved to `tasks/completed.md` with date and commit

## Repo conventions

- Keep files small and modular; extend existing module patterns rather than
  inventing second conventions.
- No `unwrap`/`expect` in non-test code (enforced by `clippy.toml`).
- This is a public repo: no secrets, placeholders, or personal content.
- Route long-running cargo/git output through `rtk` to save context.

## Phase / pillar / prompt map

| Phase | Roadmap | Pillars | Seed prompt |
| :--- | :--- | :--- | :--- |
| 0 - Spike | weeks 1-2 | pipeline proof | `prompts/prompt-00..03` (done) |
| 1 - The Gauntlet | weeks 3-4 | 05, 06, 07 | `prompts/prompt-04-gauntlet.md` (to author) |
| 2 - Context engine | weeks 5-6 | 04 | `prompts/prompt-05-context.md` (to author) |
| 3 - MCP & agentic CLI | weeks 7-8 | 09 | `prompts/prompt-06-mcp.md` (to author) |
| 4 - Ecosystem & multi-service | weeks 9-10 | 01, 02, 03 | `prompts/prompt-07-ecosystem.md` (to author) |
| 5 - WASM & mobile | weeks 11-12 | 08 | `prompts/prompt-08-wasm-mobile.md` (to author) |

---

## Phase 1 - The Gauntlet (in progress)

Goal: enforce strict quality at compile time, per `docs/pillars/07-the-gauntlet.md`,
`docs/pillars/06-matt-quality-index.md`, and `docs/pillars/05-story-to-code-traceability.md`.

### 1.1 Foundation

- [ ] Author `prompts/prompt-04-gauntlet.md` following the `prompts/` format of
  prompts 00-03 (objective, tasks, agent instructions, acceptance criteria).
- [ ] Define the Gauntlet rule interface: one small module per rule under a new
  `rivet-cli/src/gauntlet/` tree (rules, severity model, structured JSON output),
  mirroring how the parser is split into focused files.
- [ ] Wire the Gauntlet into the build pipeline between parse and generate in
  `rivet-cli/src/commands/build.rs`; failures reuse the `Diagnostic`/agentic-JSON
  path already emitted by the parser.

### 1.2 Rules

- [ ] Cyclomatic complexity walker over DSL handler bodies and DTO classes;
  fail the build above the `rivet.toml` `[gauntlet] max_complexity` threshold
  with an `E2042`-style diagnostic that carries an `ast_path`.
- [ ] Duplicate-code detector (AST hashing over the parsed module) that blocks
  redundant handlers with a pointer to both copies.
- [ ] Dead-code rule for the DSL: unused helper functions and unused DTOs are
  rejected or reported per the severity model.
- [ ] Story-to-code gate (pillar 05): every public route must carry a story ID
  unless the project config opts out; update `examples/basic/app.py` so it
  passes the default gate.
- [ ] Type-strictness rule: reject untyped/dynamic shapes that reach the IR
  (the parser already blocks most of these; the rule closes remaining gaps and
  documents the contract).

### 1.3 The MQI (Matt Quality Index)

- [ ] Implement the phase-1 subset of `rivet audit`:
  - complexity per function (High weight)
  - DSL duplicate/dead-code findings (High/Medium)
  - type strictness findings (High)
  - output an overall grade and a machine-readable breakdown, per
    `docs/pillars/06-matt-quality-index.md`.
- [ ] Decide and document how test coverage and mutation survival enter the MQI
  before phase 1 closes (they may stay Rust-side until the generated-code test
  story exists).

### 1.4 Integration and docs

- [ ] CI: run the Gauntlet on `examples/basic` in the `gauntlet-check` GitHub
  Actions job (replace the placeholder `--version` step).
- [ ] CI: add `cargo-deny` for CVE scanning (already mentioned in
  `docs/development-workflow.md`).
- [ ] Update `docs/ROADMAP.md` phase-1 checkboxes and `docs/development-workflow.md`
  as rules land; align `CONTRIBUTING.md` wording once `rivet audit` exists.

---

## Phase 2 - Context engine

Goal: persist state for humans and agents, per `docs/pillars/04-persistent-context-engine.md`.

### 2.1 Local store

- [ ] Define the `.rivet/` local store layout and the SQLite schema
  (`commands`, `sessions`, AST fingerprints per commit); pick the SQLite crate
  for `rivet-cli` and document the choice in the pillar doc.
- [ ] `rivet history`: list recorded commands with exit status and timing.
- [ ] `rivet session save`: dump the current command context to a compact
  markdown summary in the store.
- [ ] `rivet session resume`: restore a saved session as compacted markdown.

### 2.2 Semantic search

- [ ] Stand up LanceDB (local vector store) behind a small abstraction and
  index code chunks from parsed blueprints.
- [ ] `rivet explain "<symptom>"`: vector-search the code index, join AST
  fingerprints with git history, and report the likely offending commit with a
  human-readable summary.

### 2.3 Docs

- [ ] Update `docs/pillars/04-persistent-context-engine.md` and ROADMAP
  checkboxes when the stores land.

---

## Phase 3 - MCP and agentic CLI

Goal: first-class AI integration, per `docs/pillars/09-super-cli.md`.

- [ ] Research and pin a maintained MCP server SDK (e.g. the official Rust SDK
  or `rmcp`); do not reintroduce placeholder crates.
- [ ] MCP server exposing parsed AST and the vector index built in phase 2.
- [ ] Slash commands: `/plan`, `/fix`, `/trace`, backed by the MQI and
  context-engine stores.
- [ ] Auto-PR generation with performance benchmarks (`rivet /plan` producing a
  branch with tests).
- [ ] Agentic error handling: attach suggested fixes to every JSON diagnostic
  (parser diagnostics already carry them; extend to Gauntlet findings).

---

## Phase 4 - Ecosystem and multi-service

Goal: production readiness, per pillars 01, 02, and 03.

- [ ] Compile-time plugin system composed via traits (`rivet add plugin ...`
  with zero runtime overhead), pillar 01 design notes.
- [ ] Multi-service architecture switch: the same internal-channel service
  code runs in-process (monolith) or over gRPC by config, pillar 02.
- [ ] Polyglot frontend dev server: `rivet dev` proxies `/api` to the Rust
  backend and detects Vite/Rsbuild/Next.js, pillar 03.
- [ ] Production static assets embedded in the binary (rust-embed), pillar 03.
- [ ] Service discovery integration (Consul/etcd) and the built-in admin panel
  (React/Solid), per ROADMAP phase 4.
- [ ] Story-to-Jira/Linear sync (`rivet sync`), closing pillar 05's loop.

---

## Phase 5 - WASM and mobile

Goal: edge and native distribution, per `docs/pillars/08-wasm-mobile-sdk-support.md`
and `docs/pillars/data-structured-under-the-hood.md` (WASM-friendly core).

- [ ] `rivet build --target wasm` compiling to `wasm32-wasi` (Cloudflare
  Workers / Wasmtime).
- [ ] UniFFI bindings for Kotlin, Swift, and TypeScript from the Rust core.
- [ ] `rivet mobile init --platforms ios,android` generating SDKs that compile
  and pass tests.
- [ ] Rust-native feature flags from `rivet.toml` flip on in this phase or
  earlier ones: `const_generics` (fixed-size `List[T, N]` arrays) and
  `zero_copy_deserialization` (borrowed bodies), with `compile_time_rbac` and
  `raii_connections` tracked beside them.

---

## Post-v1 (parking lot)

Items intentionally out of the phase plan; pull in only with explicit scope.

- Real-time WebSocket support (plugin).
- GraphQL federation.
- Distributed tracing UI.
- Edge deployment tooling (Cloudflare Workers, Fly.io).
- Additional language front ends (Java, Go, C#).
- Performance benchmarks (`criterion`) and mutation testing that require the
  generated-code test story from phase 1.4.
