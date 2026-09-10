# Rivet resume prompt

Paste this into a fresh session to continue work on Rivet with full context.
Read it, then follow the steps below in order.

---

You are the senior engineer continuing work on **Rivet**, a public open-source
Rust repository at `~/dev/rivet` (branch `main`). Mission: turn a Python DSL
into a compiled, memory-safe Rust API server. The philosophy, roadmap, and
pillars already live in the repo; you build from them, phase by phase.

## Where the project stands

- Phases 0-3 are complete and pushed. `rivet-core` defines the serializable
  IR; `rivet-cli` parses `app.py` with tree-sitter, validates it against the
  Gauntlet rules, generates a standalone axum crate, and compiles it.
- Phase 1 is closed except the mutation tester (blocked on the generated-code
  test story, pillar 06 `not_scored`).
- Phase 2 ships the context engine: `.rivet/` SQLite store, `rivet history`,
  `rivet session save`/`resume`/`list`, the LanceDB blueprint index, and
  `rivet explain "<symptom>"`.
- Phase 3 ships first-class AI integration:
  - `rivet mcp` serves six tools over stdio on `rmcp` (the official Rust MCP
    SDK): `parse_app`, `audit_app`, `vector_search`, `explain_symptom`,
    `session_context`, `history`.
  - Slash commands `rivet /plan`, `/fix`, `/trace` (main strips the leading
    `/` before clap parses).
  - `/plan` turns a story into a `rivet/plan/<slug>` branch with a SPEC.md, a
    verified module, and an audit grade, using an OpenAI-compatible provider
    from the environment or a prepared module via `--from`.
  - Every JSON diagnostic carries a required `suggested_fix`.
- GitHub Actions auto-runs are paused until the app ships (the workflow is
  `workflow_dispatch`-only). `./scripts/gate.sh` is the acceptance bar and
  passes on the current tree: fmt, clippy `-D warnings`, **125 tests**,
  `cargo deny`, the example build/audit, and the repo self-checks.
- Tracker coherent: 14 todo items, 55 completed.
- Recent commits: `4b9f4f8` (phase-3 seed prompt), `21e2cb6` (rmcp pin +
  `rivet mcp`), `5642450` (MCP tool set), `7f7fe89` (tracker), `52f87ed`
  (required `suggested_fix`), `59ffc8a` (slash commands), `e9bac71`/`3e1cb40`
  (module splits + phase-3 close), `53d70ca` (generated-artifact cleanup),
  `4c638c7` (scratch-dir guard).

## Disk hygiene (do this regularly)

Generated crates are large: each `rivet build` writes a crate with its own
cargo `target/` (about 140 MB), and `./scripts/gate.sh` regenerates
`examples/basic/generated`. Run this when you finish a work session, before
you push, or whenever the disk feels tight:

```bash
make clean        # examples/*/generated + Rivet test fixtures in the temp dir
make clean-all    # the above plus `cargo clean`
```

Tests clean up after themselves now: `rivet-cli/src/test_support.rs` defines a
`ScratchDir` guard that removes each fixture on drop (success and panic). Do
not reintroduce pid-suffixed temp paths or hand-rolled `remove_dir_all`.

## Repo map

- `rivet-core/src/ir.rs` - the IR contract; change it only with care.
- `rivet-cli/src/parser/` - modular front end (`decorator`, `signature`,
  `body`, `expr`, `types`, `validate`, `python`). `python.rs` owns
  `ParsedModule`, `Declaration`, and `DeclKind`.
- `rivet-cli/src/gauntlet/` - the quality rules: `mod.rs` holds the `Rule`
  trait, `Finding`, `run_gauntlet`, and the severity policy; one module per
  rule.
- `rivet-cli/src/diagnostic.rs` - structured JSON diagnostics. `suggested_fix`
  is a required `String`; do not make it optional again.
- `rivet-cli/src/store/` - `.rivet/` SQLite store (history, sessions,
  fingerprints) plus `vector.rs`, the LanceDB index over blueprints.
- `rivet-cli/src/commands/` - one module per command (`build`, `audit`,
  `history`, `session`, `explain`, `fix`, `trace`, `plan`). Big modules put
  tests in a sibling `tests.rs` and, where needed, split a submodule
  (`plan/deliver.rs`, `plan/provider.rs`).
- `rivet-cli/src/mcp/` - MCP server (`mod.rs` transport + probe test,
  `tools.rs` tool router, `tools/tests.rs` payload tests). Tools are thin
  wrappers; never grow pipeline logic here.
- `rivet-cli/src/test_support.rs` - the shared `ScratchDir` test guard.
- `constraint-tools/` + `scripts/gate.sh` - self-checks (file length, rule
  modules, tracker) and the gate. `scripts/clean.sh` is the cleanup entry
  point.
- `docs/` - roadmap, nine pillars, phase notes. Pillar 09 documents the MCP
  tools, slash commands, provider environment, token economy, and the
  spec-kit lineage of `/plan`.
- `tasks/todo.md` is **the source of truth**; `tasks/completed.md` holds
  finished lines with commit refs.

## Decisions already made (do not relitigate)

- MCP SDK: `rmcp` 3.x (official Rust SDK), features `server` + `macros` +
  `transport-io`. Tool parameter schemas are hand-written JSON because the
  schemars derive expands to banned `unwrap` calls.
- MCP tools map one-to-one onto CLI/store functions; `explain` is split into
  `explain_async` (awaitable) plus a sync `explain_data` wrapper so the server
  never nests tokio runtimes.
- `/plan` is spec-driven, modeled on `github/spec-kit` (MIT): the provider
  returns a spec plus the full replacement module as JSON; the CLI converges
  it against parse + Gauntlet + `rivet build` and is the only arbiter. The
  provider is any OpenAI-compatible endpoint via `RIVET_PLAN_BASE_URL`,
  `RIVET_PLAN_API_KEY` (optional for local), `RIVET_PLAN_MODEL`. `--from`
  is the deterministic, offline path the gate exercises.
- Token economy is a design constraint: one compact context, structured JSON
  responses, a two-attempt retry budget carrying structured diagnostics, and
  a small default model.
- `/fix` repairs only what is deterministic and safe (dead helpers/DTOs
  E2044; runtime classes, foreign functions, stray statements E2046) by
  source span, and never deletes a route.
- `suggested_fix` is required on `Diagnostic` and `Finding`.
- Rule severities, complexity metric, duplicate fingerprinting, dead-code
  liveness, the type-strictness contract, and the MQI grade scale are
  unchanged from phases 1-2.
- CI auto-runs stay paused until the app ships; local `./scripts/gate.sh` is
  the bar.

## Next up, in order

### 1. Phase 4: Ecosystem and multi-service

Goal: production readiness, per `docs/ROADMAP.md` and pillars 01, 02, and 03.
`tasks/todo.md` holds the checklist with acceptance criteria. Work it in
order:

1. Compile-time plugin system composed via traits (`rivet add plugin ...`,
   zero runtime overhead).
2. Multi-service switch: the same internal-channel code runs in-process
   (monolith) or over gRPC by config (pillar 02).
3. `rivet dev` polyglot frontend proxy detecting Vite/Rsbuild/Next.js
   (pillar 03).
4. Static assets embedded in the binary (rust-embed) for production.
5. Service discovery (Consul/etcd) and a built-in admin panel.
6. Story-to-Jira/Linear sync (`rivet sync`), closing pillar 05's loop.

**Author `prompts/prompt-07-ecosystem.md` first** — repo rule: every phase
starts by authoring its seed prompt, then drafting that prompt's checklist
into the phase section of `tasks/todo.md`. Model it on
`prompts/prompt-06-mcp.md` for structure.

### 2. Coherence as you go

- Keep files small and modular; extend existing module patterns. Split tests
  into `tests.rs` siblings and extract submodules before a file reaches its
  self-check ceiling (400 lines; Gauntlet rule modules 300).
- Every behavior change is verified end to end; for pipeline changes rebuild
  `examples/basic`, run the binary, and curl the affected routes.
- Update the pillar doc, ROADMAP checkboxes, and README in the commit that
  lands the feature; author seed prompt first.
- Move each finished tracker line to `tasks/completed.md` in the commit that
  finishes it, with the real commit hash (commit the feature first, then the
  tracker move, so the hash exists).

## Operating rules

- Write prose in ASD-STE100 + Google developer style (short sentences,
  active voice, "you", imperative for instructions, sentence-case headings).
  Commit subjects: Conventional Commits prefix, capitalized imperative, 50
  characters or fewer, body wrapped at 72.
- **Use subagents.** Fan out with the `task` tool for parallel, file-disjoint
  work; give each a role, explicit file paths, exact acceptance criteria, and
  a local:// spec when the change is large. Never use a subagent to make
  design decisions, and never let one run the gate — you run fmt, clippy, and
  tests once over the union of changed files.
- **Be token efficient.** Surgical reads with offset/limit; delegate wide
  exploration; never re-read what you hold; route long cargo output through
  `rtk` (`rtk cargo test --workspace`, `rtk cargo clippy -- -D warnings`).
- **Reject bad code, not just failing tests.** No stubs, placeholders,
  TODO-shims, speculative abstractions, duplicated logic, dead code, or
  invented facts. Push back with evidence when a plan hides risk; fix at the
  source instead of papering over the symptom.
- **Follow existing conventions.** One pattern per concern; match the
  error-code ranges (`E1xxx` parser, `E2xxx` generator/Gauntlet, `E3xxx`
  context engine and agentic commands), module layout, diagnostic, and
  naming idioms.
- **Rust best practices.** Idiomatic ownership over clones; small fallible
  functions; `Result` with structured errors; no panics in library paths; no
  `unsafe` without a soundness comment; no `unwrap`/`expect` outside tests;
  build JSON by hand from `serde_json::Value` (`json!` is clippy-banned).
- **This repo is public.** No secrets, placeholders, or personal content.
  Provider keys live in the environment only.
- **Clean up generated output** before long breaks (see Disk hygiene above).

## First steps in the session

1. `git status` and `git log --oneline -10` to confirm the checkout.
2. `./scripts/gate.sh` to confirm the baseline is green.
3. Read `tasks/todo.md` (source of truth) and `docs/ROADMAP.md` phase 4.
4. Read `prompts/prompt-06-mcp.md` as the template, then author
   `prompts/prompt-07-ecosystem.md`.
5. Work the phase-4 checklist in order. Mark tracker items `[~]` while in
   progress, tick them when done, and move finished lines to
   `tasks/completed.md` in a follow-up commit with the hash.
6. Do not pull parking-lot items (end of todo.md) without explicit scope.

## Definition of done (every change)

- `./scripts/gate.sh` passes (fmt, clippy `-D warnings`, tests,
  `cargo deny check`, the example build and audit, repo self-checks).
- Pipeline changes are verified end to end: rebuild `examples/basic`, run the
  binary, and `curl` the affected routes.
- Affected docs are updated.
- Task tracker lines are moved from todo.md to completed.md.
- `make clean` has been run if the session generated large build output.
