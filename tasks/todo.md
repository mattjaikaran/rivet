# Rivet task tracker

This file is the source of truth for what needs to be done. It follows the
[roadmap](docs/ROADMAP.md) phases and the original `prompts/` build order, and
each phase references the pillars it serves.

Last updated: 2026-09-10

## Scope: finish the Python front end first

Rivet ingests a Python/TypeScript DSL (`docs/ARCHITECTURE.md`). Only the
Python front end exists today. Finish every Python-side item before you
start TypeScript work.

- In scope: the Python DSL front end, the rest of phase 4, and the phase-5
  work that exports a Python-authored app (WASM, the Kotlin/Swift bindings,
  the `rust_native_features` flags).
- Deferred: a TypeScript DSL front end, and the UniFFI TypeScript (React
  Native) binding in phase 5. Do not start either without explicit scope.
- The phase-4 admin panel is not TypeScript work: it ships as one static
  file, with no React or Solid build step.

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
- Run `make clean` after a session that generated crates; tests clean their
  own fixtures through the `ScratchDir` guard, so do not add pid-suffixed
  temp paths.

## Phase / pillar / prompt map

| Phase | Roadmap | Pillars | Seed prompt |
| :--- | :--- | :--- | :--- |
| 0 - Spike | weeks 1-2 | pipeline proof | `prompts/prompt-00..03` (done) |
| 1 - The Gauntlet | weeks 3-4 | 05, 06, 07 | `prompts/prompt-04-gauntlet.md` (done) |
| 2 - Context engine | weeks 5-6 | 04 | `prompts/prompt-05-context.md` (done) |
| 3 - MCP & agentic CLI | weeks 7-8 | 09 | `prompts/prompt-06-mcp.md` (done) |
| 4 - Ecosystem & multi-service | weeks 9-10 | 01, 02, 03 | `prompts/prompt-07-ecosystem.md` (done) |
| 5 - WASM & mobile | weeks 11-12 | 08 | `prompts/prompt-08-wasm-mobile.md` (to author) |

---

## Cross-phase: Rust-native features

`rivet.toml` `[rust_native_features]` advertises four flags. Each is false
until its feature is actually implemented, and the example config flips a
flag only then, so the config never over-claims.

`const_generics` has landed and its flag is true in the example config; see
`tasks/completed.md`. The other three stay false, because each needs a layer
the generator does not have yet:

- [ ] `zero_copy_deserialization`: borrowed request bodies (`&str` fields via
  `#[serde(borrow)]`). The IR tracks `FieldDefinition::is_borrowed`, but the
  parser never sets it and no renderer emits a lifetime, so the flag would
  claim a plumbing the IR does not carry.
  Acceptance: a DTO with a borrowed `str` field builds and a request round-trips
  without owned copies; `rivet.toml` flag true.
- [ ] `raii_connections`: pooled connections released automatically at handler
  exit. The DSL has no database surface at all yet, so there is no
  connection to pool.
  Acceptance: a handler that touches the database compiles with no explicit
  close calls; a test proves the connection returns to the pool; flag true.
- [ ] `compile_time_rbac`: typestate auth so protected routes require an
  authenticated request type at compile time. The decorator accepts only
  `path` and `stories`, so there is no way to mark a route protected.
  Acceptance: a route flagged protected fails to compile without an
  authentication layer; the generated code has no runtime role lookup; flag true.
- The completed `const_generics` line is in `tasks/completed.md`.

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

## Phase 2 - Context engine (complete 2026-09-09)

Goal: persist state for humans and agents, per
`docs/pillars/04-persistent-context-engine.md`.

All sections (2.1 store, 2.2 semantic search, 2.3 docs) are done; see
`tasks/completed.md`. The phase-2 ROADMAP checkboxes are ticked and the
SQLite/LanceDB choices are recorded in pillar 04.

---

## Phase 3 - MCP and agentic CLI (complete 2026-09-09)

Goal: first-class AI integration, per `docs/pillars/09-super-cli.md`.

All five items are done; see `tasks/completed.md`. The MCP server runs
`rivet mcp` over stdio with rmcp; the slash commands land auto-PR
generation and required `suggested_fix` diagnostics.

---

## Phase 4 - Ecosystem and multi-service (complete 2026-09-10)

Goal: production readiness, per pillars 01, 02, 03, and 05. Seed:
`prompts/prompt-07-ecosystem.md`.

All seven deliverables are done: the compile-time plugin system, the
multi-service transport switch, the polyglot `rivet dev` proxy, the embedded
static assets, the admin panel, service discovery (Consul and etcd), and
`rivet sync` (Jira and Linear). See `tasks/completed.md`; the phase-4
ROADMAP boxes are ticked, and pillars 01, 02, 03, and 05 describe what
shipped.

---

## Phase 5 - WASM and mobile

Goal: edge and native distribution, per
`docs/pillars/08-wasm-mobile-sdk-support.md` and
`docs/pillars/data-structured-under-the-hood.md` (WASM-friendly core). Seed:
`prompts/prompt-08-wasm-mobile.md`.

- [ ] UniFFI bindings for Kotlin and Swift from the core. The TypeScript
  (React Native) binding stays deferred until the Python front end is done
  (see the scope section).
  Acceptance: each generated binding compiles against the fixture core.
  Blocked: no JDK, `kotlinc`, Android SDK, `uniffi-bindgen`, or Xcode
  command-line tools in this repository.
- [ ] `rivet mobile init --platforms ios,android` producing SDKs that compile
  and pass tests.
  Acceptance: the generated projects build in the platform toolchains in CI.
  Blocked: the same toolchains as the bindings above.
- [ ] Flip the cross-phase `rust_native_features` flags as their features land
  (see the cross-phase section).
  Acceptance: all four flags are true in the example config and no generator
  error path for them remains.

Toolchain note: the WASM target is verifiable in this repository —
`rustup target add wasm32-wasip1` and `wasmtime` are installed, and the gate
runs the generated module. The mobile lines are not: they need a JDK,
`kotlinc`, the Android SDK, `uniffi-bindgen`, and Xcode's command-line tools.
Record a mobile line as blocked with the toolchain it needs rather than
ticking it.

---

## Post-v1 (parking lot)

Items intentionally out of the phase plan; pull in only with explicit scope.
Each needs its acceptance drafted when pulled.

- Real-time WebSocket support (plugin).
- GraphQL federation.
- Distributed tracing UI.
- Edge deployment tooling (Cloudflare Workers, Fly.io).
- A TypeScript DSL front end. `docs/ARCHITECTURE.md` and
  `prompts/prompt-01-ir.md` name it as a co-equal parser over the same IR,
  so the Python front end comes first. Do not shape the IR for it without
  explicit scope.
- Additional language front ends (Java, Go, C#).
- Performance benchmarks (`criterion`) and mutation testing that require the
  generated-code test story from phase 1.3.
