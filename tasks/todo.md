# Rivet task tracker

This file is the source of truth for what needs to be done. It follows the
[roadmap](docs/ROADMAP.md) phases and the original `prompts/` build order, and
each phase references the pillars it serves.

Last updated: 2026-09-09

## How to use this file

- Keep `tasks/todo.md` authoritative for outstanding work.
- When a task is finished: tick it, add the date and commit, then move the
  line to `tasks/completed.md` in the same change.
- Group work by roadmap phase. Never jump ahead of the phase the session is in
  unless the task says otherwise.
- Every phase starts by authoring its `prompts/prompt-NN-*.md` seed so future
  sessions inherit the same structure the first prompts gave this repo, then
  drafting that prompt's checklist into the phase section here.
- Checkboxes: `- [ ]` open, `- [x]` done (awaiting move), `- [~]` in progress.
- Every item carries a one-line acceptance criterion. An item is done only
  when its acceptance is demonstrated.

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
| 1 - The Gauntlet | weeks 3-4 | 05, 06, 07 | `prompts/prompt-04-gauntlet.md` (done) |
| 2 - Context engine | weeks 5-6 | 04 | `prompts/prompt-05-context.md` (done) |
| 3 - MCP & agentic CLI | weeks 7-8 | 09 | `prompts/prompt-06-mcp.md` (to author) |
| 4 - Ecosystem & multi-service | weeks 9-10 | 01, 02, 03 | `prompts/prompt-07-ecosystem.md` (to author) |
| 5 - WASM & mobile | weeks 11-12 | 08 | `prompts/prompt-08-wasm-mobile.md` (to author) |

---

## Cross-phase: Rust-native features

`rivet.toml` `[rust_native_features]` advertises four flags. Each is false
today; each item below flips its flag to true in the example config only when
the feature is actually implemented, so the config never over-claims.

- [ ] `zero_copy_deserialization`: borrowed request bodies (`&str` fields via
  `#[serde(borrow)]`).
  Acceptance: a DTO with a borrowed `str` field builds and a request round-trips
  without owned copies; `rivet.toml` flag true.
- [ ] `raii_connections`: pooled connections released automatically at handler
  exit.
  Acceptance: a handler that touches the database compiles with no explicit
  close calls; a test proves the connection returns to the pool; flag true.
- [ ] `compile_time_rbac`: typestate auth so protected routes require an
  authenticated request type at compile time.
  Acceptance: a route flagged protected fails to compile without an
  authentication layer; the generated code has no runtime role lookup; flag true.
- [ ] `const_generics`: fixed-size arrays from `List[T, N]` generate
  `[T; N]` instead of erroring with `E2003`.
  Acceptance: a DTO with `List[float, 768]` generates `[f64; 768]` and builds;
  flag true.

---

## Phase 1 - The Gauntlet (complete 2026-09-09)

Goal: enforce strict quality at compile time, per `docs/pillars/07-the-gauntlet.md`,
`docs/pillars/06-matt-quality-index.md`, and `docs/pillars/05-story-to-code-traceability.md`.

All sections (1.1 foundation, 1.2 rules, 1.3 the MQI, 1.4 integration
and docs) are complete; see `tasks/completed.md`. The mutation-tester
roadmap bullet stays open: it is blocked on the generated-code test
story (see pillar 06, `not_scored`).

---

## Constraint tools: gate the Rivet repo itself (complete 2026-09-09)

The Gauntlet gates DSL apps; this workstream gates the Rivet source tree the
same way. Deterministic, small, binary pass/fail tools chained in one gate,
after the SwarmForge pattern in
`~/dev/django-ninja-boilerplate/docs/CONSTRAINT_TOOLS.md`. Not a roadmap
phase; it precedes phase 2 so later work inherits the self-checks.

All four items are done; see `tasks/completed.md`. The mutation-tester
parking-lot bullet stays open (generated-code test story, pillar 06).

---

## Phase 2 - Context engine

Goal: persist state for humans and agents, per
`docs/pillars/04-persistent-context-engine.md`.

### 2.1 Local store

- [ ] Define the `.rivet/` store layout and SQLite schema (`commands`,
  `sessions`, AST fingerprints per commit); choose and document the SQLite
  crate for `rivet-cli`.
  Acceptance: the schema migration runs clean on an empty store and the choice
  is recorded in the pillar doc.
- [ ] `rivet history`: list recorded commands with exit status and timing.
  Acceptance: two recorded builds appear in order with correct statuses.
- [ ] `rivet session save`: dump current command context to compact markdown in
  the store.
  Acceptance: the saved session file round-trips and reads back as markdown.
- [ ] `rivet session resume`: restore a saved session as compacted markdown.
  Acceptance: resuming prints the same context the save produced.

### 2.2 Semantic search

- [ ] Stand up LanceDB behind a small abstraction and index parsed blueprints.
  Acceptance: an index test searches a known chunk and ranks it first.
- [ ] `rivet explain "<symptom>"`: vector search plus AST fingerprints and git
  history to report the likely offending commit.
  Acceptance: the command on a fixture returns the introducing commit with a
  human-readable summary.

### 2.3 Docs

- [ ] Update `docs/pillars/04-persistent-context-engine.md` and ROADMAP
  checkboxes as stores land.
  Acceptance: shipped features map one-to-one to ticked roadmap items.

---

## Phase 3 - MCP and agentic CLI

Goal: first-class AI integration, per `docs/pillars/09-super-cli.md`.

- [ ] Research and pin a maintained MCP server SDK (official Rust SDK or
  `rmcp`); no placeholder crates.
  Acceptance: the crate resolves from the registry and a hello-world MCP tool
  answers a probe request.
- [ ] MCP server exposing parsed AST and the phase-2 vector index.
  Acceptance: an MCP client lists the AST/vector tools and gets valid data.
- [ ] Slash commands `/plan`, `/fix`, `/trace` backed by MQI and context
  stores.
  Acceptance: each command has an integration test with a fixture project.
- [ ] Auto-PR generation with passing tests (`rivet /plan`).
  Acceptance: on a fixture story the command produces a branch whose tests
  pass.
- [ ] Agentic error handling: suggested fixes on every JSON diagnostic,
  including Gauntlet findings.
  Acceptance: every emitted diagnostic in an error scenario carries
  `suggested_fix`.

---

## Phase 4 - Ecosystem and multi-service

Goal: production readiness, per pillars 01, 02, and 03.

- [ ] Compile-time plugin system composed via traits
  (`rivet add plugin ...`, zero runtime overhead).
  Acceptance: a fixture plugin compiles in with no registry lookup and its
  route answers a request.
- [ ] Multi-service switch: same internal-channel code runs in-process
  (monolith) or over gRPC by config (pillar 02).
  Acceptance: one blueprint builds and runs both topologies; the integration
  test exercises both.
- [ ] `rivet dev` polyglot frontend proxy detecting Vite/Rsbuild/Next.js
  (pillar 03).
  Acceptance: with a fixture Vite app, `/api/*` reaches the Rust backend and
  other paths reach the dev server.
- [ ] Static assets embedded in the binary (rust-embed) for production.
  Acceptance: a built binary serves a fixture `dist/` file without a filesystem.
- [ ] Service discovery (Consul/etcd) and built-in admin panel (React/Solid).
  Acceptance: a two-service compose stack registers and the panel lists routes.
- [ ] Story-to-Jira/Linear sync (`rivet sync`), closing pillar 05's loop.
  Acceptance: a dry run against a mock API reports the expected story diff.

---

## Phase 5 - WASM and mobile

Goal: edge and native distribution, per
`docs/pillars/08-wasm-mobile-sdk-support.md` and
`docs/pillars/data-structured-under-the-hood.md` (WASM-friendly core).

- [ ] `rivet build --target wasm` compiling to `wasm32-wasi`.
  Acceptance: a generated app runs under Wasmtime and answers the ping route.
- [ ] UniFFI bindings for Kotlin, Swift, and TypeScript from the core.
  Acceptance: each generated binding compiles against the fixture core.
- [ ] `rivet mobile init --platforms ios,android` producing SDKs that compile
  and pass tests.
  Acceptance: the generated projects build in the platform toolchains in CI.
- [ ] Flip the cross-phase `rust_native_features` flags as their features land
  (see the cross-phase section).
  Acceptance: all four flags are true in the example config and no generator
  error path for them remains.

---

## Post-v1 (parking lot)

Items intentionally out of the phase plan; pull in only with explicit scope.
Each needs its acceptance drafted when pulled.

- Real-time WebSocket support (plugin).
- GraphQL federation.
- Distributed tracing UI.
- Edge deployment tooling (Cloudflare Workers, Fly.io).
- Additional language front ends (Java, Go, C#).
- Performance benchmarks (`criterion`) and mutation testing that require the
  generated-code test story from phase 1.3.
