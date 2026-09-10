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
- Phase 3 ships first-class AI integration: `rivet mcp` (six tools over stdio
  on `rmcp`), the slash commands `rivet /plan`, `/fix`, `/trace`, and a
  required `suggested_fix` on every JSON diagnostic.
- Phase 4 is in progress and four of its deliverables have landed:
  - **Plugin system** (`53242fd`): `rivet-plugin-api` holds the `Plugin`
    trait and a monomorphized `install`; `rivet add plugin` writes the
    `[plugins]` table; `generate_project` adds one dependency and one install
    call per plugin. Reference plugin: `examples/basic/plugins/auth-token`.
  - **Multi-service transport** (`a675668`): the generated crate splits into a
    transport-free service layer and an internal channel; `[transport] mode`
    selects an in-process direct call or gRPC.
  - **Polyglot dev proxy** (`44345fd`): `rivet dev` detects Vite, Rsbuild,
    Next.js, or Webpack from the config file, serves the blueprint's routes
    and `/api/*` from the backend (prefix stripped), sends every other path to
    the frontend dev server, and tunnels the frontend's HMR upgrade.
  - **Embedded static assets** (`7a3646f`): `[frontend] dist` compiles the
    production build into the generated crate with `rust-embed`; the router
    mounts it as the fallback, so a blueprint route wins. The single-page rule
    mirrors the dev server's rewrite (a `GET`/`HEAD` whose path names no file,
    from a client that accepts `text/html`), so an `XHR` miss keeps its `404`.
    Every asset carries a strong `ETag`; `If-None-Match` answers `304`. A
    missing `dist` warns with `E2004` and still builds. The router compresses
    every response with `tower-http` Brotli.
- GitHub Actions auto-runs are paused until the app ships (the workflow is
  `workflow_dispatch`-only). `./scripts/gate.sh` is the acceptance bar:
  fmt, clippy `-D warnings`, **198 tests**, `cargo deny`, the example
  build/audit, and the repo self-checks.
- Tracker coherent: 11 todo items, 83 completed.
- Recent commits: `7a3646f` (embedded assets), `6e0c6e0` (resume prompt),
  `735a100` (dev-proxy tracker move), `44345fd` (`rivet dev`), `a675668`
  (transport switch), `53242fd` (plugin system).

## Disk hygiene (do this)

`rivet build` writes a generated crate with its own cargo `target/` (about
140 MB per app), and `./scripts/gate.sh` regenerates `examples/basic/generated`.
The cargo cache is warm from the asset session, so `./scripts/gate.sh` takes
about a minute. After a `make clean-all` the first release build costs about
7 minutes and the first test build about 4; prefer `target/debug/rivet` while
you iterate.
```bash
make clean        # examples/*/generated + Rivet test fixtures in the temp dir
make clean-all    # the above plus `cargo clean` (about 12 GiB)
```

The last session ended with `make clean-all`, so `target/` is gone: the first
release build takes about 7 minutes, and the first test build about 4. Budget
for it, and prefer `target/debug/rivet` while you iterate.

Tests clean up after themselves: `rivet-cli/src/test_support.rs` defines a
`ScratchDir` guard that removes each fixture on drop (success and panic). Do
not reintroduce pid-suffixed temp paths or hand-rolled `remove_dir_all`.

Never commit build output. `.gitignore` covers `target/` and
`examples/*/generated/`; verify with `git status --short` before every commit.

## Repo map

- `rivet-core/src/ir.rs` - the IR contract; change it only with care.
- `rivet-cli/src/parser/` - modular front end (`decorator`, `signature`,
  `body`, `expr`, `types`, `validate`, `python`). `python.rs` owns
  `ParsedModule`, `Declaration`, and `DeclKind`.
- `rivet-cli/src/gauntlet/` - the quality rules: `mod.rs` holds the `Rule`
  trait, `Finding`, `run_gauntlet`, and the severity policy; one module per
  rule.
- `rivet-cli/src/commands/` - one module per command (`build`, `audit`,
  `history`, `session`, `explain`, `fix`, `trace`, `plan`, `add`, `dev`).
  Big modules put tests in a sibling `tests.rs` and split submodules before a
  file reaches the 400-line ceiling: `plan/deliver.rs`, `plan/provider.rs`,
  `dev/routing.rs`, `dev/tunnel.rs`.
- `rivet-cli/src/transpiler/rust.rs` + `rust/` - the generator: `handler.rs`
  (thin axum handlers over the channel), `service.rs` (transport-free route
  logic), `channel.rs` (the typed in-process/gRPC channel), `assets.rs` (the
  embedded frontend build, its router fallback, and the compression layer).
- `rivet-cli/src/mcp/` - MCP server (`mod.rs` transport + probe test,
  `tools.rs` tool router, `tools/tests.rs` payload tests). Tools are thin
  wrappers; never grow pipeline logic here.
- `rivet-cli/src/test_support.rs` - the shared `ScratchDir` test guard.
- `constraint-tools/` + `scripts/gate.sh` - self-checks (file length, rule
  modules, tracker) and the gate. `scripts/clean.sh` is the cleanup entry
  point.
- `docs/` - roadmap, nine pillars, phase notes. Pillar 03 documents `rivet
  dev` and the embedded assets; pillar 09 documents the MCP tools and slash
  commands.
- `tasks/todo.md` is **the source of truth**; `tasks/completed.md` holds
  finished lines with commit refs.

## Decisions already made (do not relitigate)

- MCP SDK: `rmcp` 3.x, features `server` + `macros` + `transport-io`. Tool
  parameter schemas are hand-written JSON because the schemars derive expands
  to banned `unwrap` calls.
- `/plan` is spec-driven, modeled on `github/spec-kit` (MIT). The provider is
  any OpenAI-compatible endpoint; `--from` is the deterministic offline path.
- `/fix` repairs only what is deterministic and safe, and never deletes a
  route.
- `suggested_fix` is required on `Diagnostic` and `Finding`.
- The generated backend mounts the blueprint's own route paths and nothing
  else. `/api` is a proxy-level convenience only: `rivet dev` strips it before
  the request goes upstream. Do not add an `/api` mount to the generator.
- The dev proxy tunnels upgrades over a raw connection (hyper `on_upgrade` +
  `copy_bidirectional`); an HTTP client cannot carry a websocket handshake.
- Rule severities, the complexity metric, duplicate fingerprinting, dead-code
  liveness, the type-strictness contract, and the MQI grade scale are
  unchanged from phases 1-2.
- CI auto-runs stay paused until the app ships; local `./scripts/gate.sh` is
  the bar.
- `[frontend]` owns the production build (`dist`, `spa`), not a separate
  `[assets]` table: `rivet dev` already owns the frontend, and the phase-4
  seed's original `[assets] dir` shape is superseded.
- The embedded single-page fallback stays a *navigation* rule (no file
  extension, `Accept: text/html`). The generated binary also runs as the
  `rivet dev` backend, so a blanket `index.html` fallback would answer API
  typos with HTML.
- The generated router compresses responses with `tower-http`'s
  `CompressionLayer` and the Brotli feature. Keep it: pillar 03 promised
  Brotli, and wire compression costs no binary size.
- Generated-app tests build a real crate inside a `ScratchDir`
  (`rivet-cli/src/commands/build/tests.rs`); fixtures the test mutates (a
  `dist/` it renames) live there too, never in the repo — except the example
  fixture under `examples/basic/dist/`, which `.gitignore` now excepts.

## Next up, in order

Finish phase 4, per `docs/ROADMAP.md` and `tasks/todo.md` (work the checklist
in order; each line carries its acceptance criterion):

1. **Service discovery (Consul/etcd) and the admin panel.** Acceptance:
   registration posts the service and port to a stub registry in a test, and
   `/__rivet/routes` lists the routes the app serves. The route list is
   generator work beside `mod assets`; the registry client is new.
2. **Story-to-Jira/Linear sync (`rivet sync`).** Acceptance:
   `rivet sync --dry-run` reports the expected story diff from a captured
   tracker payload and writes nothing. Closes pillar 05's loop.
3. **Phase-4 docs and tracker close.** Finish pillars 01-03, tick the ROADMAP
   phase-4 boxes, refresh the README, and move every finished line to
   `tasks/completed.md` with its commit hash.

Then phase 5 (WASM and mobile), which starts by authoring
`prompts/prompt-08-wasm-mobile.md`.

## How to work (house rules)

- **Use subagents.** Fan out with the `task` tool for parallel, file-disjoint
  work: give each subagent a role, explicit file paths, exact acceptance
  criteria, and a `local://` spec when the change is large. A read-only
  question about unfamiliar code goes to a `scout` subagent instead of a chain
  of reads. Never let a subagent make a design decision, and never let one run
  the gate. If a spawn fails with `No model selected`, that is an environment
  fault, not a task fault: work the slices inline and keep the same file
  discipline.
- **Save context with `rtk`.** Route long output through it:
  `rtk cargo test --workspace`, `rtk cargo clippy -- -D warnings`,
  `rtk ./scripts/gate.sh`, `rtk git log`. Use surgical reads
  (`read` with `offset`/`limit`) and never re-read what you hold.
- **Commit in small, coherent units, and commit when you finish one.** One
  commit per unit (feature, docs, tracker move), Conventional Commits prefix,
  capitalized imperative subject, 50 characters or fewer, body wrapped at 72.
  Commit the feature first, then the tracker move, so the hash exists when the
  tracker line records it.
- **Verify before you commit.** Run the specific test or smoke test that
  covers the change; run `./scripts/gate.sh` once over the union of the
  session's changes, not per file.
- **Clean up generated output before you stop** (`make clean`, or
  `make clean-all` when disk is tight). Never leave `target/` or
  `examples/*/generated/` in a commit.
- **Reject bad code, not just failing tests.** No stubs, placeholders,
  TODO-shims, speculative abstractions, duplicated logic, dead code, or
  invented facts. Fix at the source instead of papering over the symptom.
- **Follow existing conventions.** One pattern per concern; match the
  error-code ranges (`E1xxx` parser, `E2xxx` generator/Gauntlet, `E3xxx`
  context engine and agentic commands), module layout, diagnostic, and naming
  idioms.
- **Rust best practices.** Idiomatic ownership over clones; small fallible
  functions; `Result` with structured errors; no panics in library paths; no
  `unsafe` without a soundness comment; no `unwrap`/`expect` outside tests;
  build JSON by hand from `serde_json::Value` (`json!` is clippy-banned).
- **This repo is public.** No secrets, placeholders, or personal content.
  Provider keys live in the environment only.

## First steps in the session

1. `git status` and `rtk git log --oneline -10` to confirm the checkout.
2. `rtk ./scripts/gate.sh` to confirm the baseline is green.
3. Read `tasks/todo.md` (source of truth) and `docs/ROADMAP.md` phase 4.
4. Start item 1 (service discovery and the admin panel): read the generated
   route table and pillar 02 first, design it, then decompose it into
   subagent-sized, file-disjoint tasks.
5. Mark tracker items `[~]` while in progress, then move the finished line to
   `tasks/completed.md` with its commit hash in a follow-up commit.
   `check-tracker` rejects any `[x]` left in `todo.md`, so never tick a line
   in place.
6. Do not pull parking-lot items (end of `tasks/todo.md`) without explicit
   scope.

## Definition of done (every change)

- `./scripts/gate.sh` passes (fmt, clippy `-D warnings`, tests,
  `cargo deny check`, the example build and audit, repo self-checks).
- Pipeline changes are verified end to end: rebuild `examples/basic`, run the
  binary, and `curl` the affected routes.
- Affected docs are updated (pillar doc, ROADMAP boxes, README).
- Task tracker lines are moved from `todo.md` to `completed.md`.
- `make clean` has been run if the session generated large build output.
