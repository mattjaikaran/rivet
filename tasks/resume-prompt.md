# Rivet resume prompt

Paste this into a fresh session to continue work on Rivet with full context.
Read it, then follow the steps below in order.

---

You are the senior engineer continuing work on **Rivet**, a public open-source
Rust repository at `~/dev/rivet` (branch `main`). Mission: turn a Python DSL
into a compiled, memory-safe Rust API server. The philosophy, roadmap, and
pillars already live in the repo; you build from them, phase by phase.

## Where the project stands

- Phases 0, 1, and 2 are complete. `rivet-core` defines the serializable IR;
  `rivet-cli` parses `app.py` with tree-sitter, validates it against the
  Gauntlet rules, generates a standalone axum crate, and compiles it.
- The mutation tester stays open until the generated-code test story exists
  (pillar 06, `not_scored`). Constraint tools gate the Rivet source tree and
  are done.
- Phase 2 (the context engine) ships: `.rivet/` SQLite store, `rivet history`,
  `rivet session save`/`resume`/`list`, the LanceDB blueprint index, and
  `rivet explain "<symptom>"`. See `tasks/completed.md` section 2.
- GitHub Actions auto-runs are paused until the app ships (workflow is
  `workflow_dispatch`-only). `./scripts/gate.sh` is the acceptance bar and
  passes on the current tree: fmt, clippy `-D warnings`, 106 tests,
  `cargo deny`, the example build/audit, and the repo self-checks.
- Tracker coherent: 19 todo items, 46 completed.
- Recent commits: `38cdb1d` (SQLite/LanceDB deps, MSRV 1.91), `617632e`
  (store + history + session), `68e93f9` (RFC 3339 history), `8dd08e7`
  (vector index + explain), `bd3443d` (phase-2 close), `b56b66a` (crate-doc
  nit), `2c60876` (protoc for CI), `04b6119` (CI pause).

## Repo map

- `rivet-core/src/ir.rs` - the IR contract; change it only with care.
- `rivet-cli/src/parser/` - modular front-end files (`decorator`, `signature`,
  `body`, `expr`, `types`, `validate`, `python`). `python.rs` owns
  `ParsedModule`, `Declaration`, and `DeclKind` (Route, Helper, Foreign, Dto,
  RuntimeClass, Other).
- `rivet-cli/src/gauntlet/` - the quality rules: `mod.rs` holds the `Rule`
  trait, `Finding`, `run_gauntlet`, and the severity policy; one module per
  rule documents its metric and contract in its own doc comment.
- `rivet-cli/src/diagnostic.rs` - structured JSON diagnostics; `Severity` is
  `Warning` or `Blocker`. JSON is built by hand from `serde_json::Value` —
  the `json!` macro expands to `unwrap` calls, which clippy bans.
- `rivet-cli/src/config.rs` - `RivetConfig` with the `[gauntlet]` section.
- `rivet-cli/src/store/` - the `.rivet/` SQLite store (command history,
  sessions, module digests) plus `vector.rs`, the local LanceDB embedding
  index over blueprints. Recorded in pillar 04.
- `rivet-cli/src/commands/` - one module per CLI command (`build`, `audit`,
  `history`, `session`, `explain`). Each returns structured
  `Diagnostic`s on failure.
- `rivet-cli/src/main.rs` - clap command enum; failures print every
  diagnostic as JSON plus a summary line.
- `constraint-tools/` + `scripts/gate.sh` - repo self-checks (file-length,
  rule-module coherence, tracker coherence) and the gate that chains fmt,
  clippy, tests, deny, the example build/audit, and the self-checks.
- `deny.toml` + `.github/workflows/ci.yml` - dependency CVE scanning and the
  three CI jobs (`rust-checks`, `cargo-deny`, `gauntlet-check`); auto-runs
  are paused until the app ships.
- `docs/` - roadmap, nine pillars, phase notes. Pillar 06 documents the MQI
  grade scale, weights, and the coverage/mutation `not_scored` decision;
  pillar 07 documents the shipped rules and severity model.
- `prompts/` - phase seed prompts (00-05 done; each new phase authors its
  next prompt first).
- `tasks/todo.md` - **the source of truth** for outstanding work.
- `tasks/completed.md` - finished tasks, moved from todo.md with commit refs.

## Decisions already made (do not relitigate)

- Rule severities: `complexity`, `story_link`, `type_strictness` are always
  blockers when enabled; `duplicate_code` (blocker) and `dead_code`
  (warning) are configurable to `warn` or `block`.
- Complexity metric: base 1 plus one point per
  `if`/`elif`/`for`/`while`/`and`/`or`/ternary/`match`-arm node.
- Duplicate fingerprint: parameters + return annotation + body text,
  whitespace-normalized.
- Dead-code liveness: a helper is live only when another module function
  refers to it; a DTO is live when the blueprint reachability closure holds it.
- Type-strictness contract: dynamic typing is allowed only at the documented
  JSON boundary; runtime classes, foreign decorators, extra decorators on
  handlers, and stray module statements fail `E2046`.
- MQI grade scale: A+ to F over 11 bands; each scored dimension starts at
  100, a blocker finding subtracts 20, a warning 10, floor 0; High = 3,
  Medium = 2 weights; overall is the weighted mean, one decimal. Coverage,
  mutation survival, and doc coverage are `not_scored` until the
  generated-code test story exists (pillar 06).
- The `examples/basic` routes carry `US-001` and `US-002` so the example
  passes the default story gate.
- The `.rivet/` store lives next to the app module (SQLite via bundled
  rusqlite for portability; idempotent migration keyed on
  `PRAGMA user_version`); history records every invocation before it runs and
  finishes it with exit status and duration, newest first. Sessions save by
  name and overwrite on re-save.
- Semantic search uses a local LanceDB table of blueprint embeddings with a
  hashed module digest stored per commit; `rivet explain "<symptom>"`
  embeds the symptom, finds the nearest route, and reports the introducing
  commit via git pickaxe on the handler. Commit 8dd08e7 records the design.
- CI auto-runs stay paused until the app ships; local `./scripts/gate.sh`
  is the bar. Re-enable by restoring the push and pull_request triggers in
  `.github/workflows/ci.yml`.

## Next up, in order

### 1. Phase 3: MCP and agentic CLI

Goal: first-class AI integration, per `docs/pillars/09-super-cli.md` and the
phase-3 roadmap section. The todo.md phase-3 checklist already lists five
items with acceptances; draft nothing new until the seed prompt exists.

**Author `prompts/prompt-06-mcp.md` first** — repo rule: every phase starts
by authoring its seed prompt, then drafts that prompt's checklist into the
phase section of `tasks/todo.md`. Model the seed on
`prompts/prompt-05-context.md` (structure, acceptance style, references to
the pillars it serves) and read `docs/pillars/09-super-cli.md` plus the
phase-2 commits so the MCP server can expose what phase 2 built.

Then work the checklist in order:

1. **MCP server SDK**: research and pin a maintained Rust SDK (official SDK
   or `rmcp`); no placeholder crates. A hello-world MCP tool must answer a
   probe request.
2. **MCP server**: expose the parsed AST and the phase-2 vector index over
   MCP; a client must list the tools and get valid data.
3. **Slash commands** `/plan`, `/fix`, `/trace` backed by the MQI and the
   context stores; each command carries an integration test on a fixture
   project.
4. **Auto-PR generation** (`rivet /plan` on a fixture story) producing a
   branch whose tests pass.
5. **Agentic error handling**: every emitted diagnostic in an error scenario
   carries `suggested_fix`, including Gauntlet findings.

Update pillar 09, the ROADMAP checkboxes, and the README in the commit that
lands each feature; move each finished tracker line to `tasks/completed.md`
in the same commit.

### 2. Coherence as you go

- Keep files small and modular; extend existing module patterns.
- Every behavior change is verified end to end on `examples/basic`
  (rebuild, run, `curl` the affected routes) before you commit it.
- Update pillar docs, ROADMAP checkboxes, and the README in the commit that
  lands the feature.
- Move each finished tracker line to `tasks/completed.md` in the commit that
  finishes it, with the commit hash.

## Operating rules

- Write prose in ASD-STE100 + Google developer style (short sentences,
  active voice, "you", imperative for instructions). Commit subjects:
  Conventional Commits prefix, capitalized imperative, under 50 characters,
  body wrapped at 72.
- **Use subagents.** Fan out with the `task` tool for parallel, file-disjoint
  work (independent checks, docs vs code, separate command modules). Give
  each subagent a role, explicit file paths, and its acceptance criteria;
  never use a subagent to make design decisions. Subagents do not run gates —
  you run fmt, clippy, and tests once over the union of changed files.
- **Route long cargo output through `rtk`** (`rtk cargo test --workspace`,
  `rtk cargo clippy -- -D warnings`) to save context.
- **Make multiple focused commits**, one concern each; push when the work is
  finished.
- Use `search`, `read`, and the language tools for investigation; do not
  shell out to grep or sed for content.
- Never use `unwrap`/`expect` in non-test code (enforced by clippy). Build
  JSON by hand from `serde_json::Value`; `json!` is banned by clippy.
- This repo is public. No secrets, placeholders, invented emails, or content
  that would embarrass the project. Nothing gets committed that you would
  not publish.
- `examples/basic/generated` is gitignored output; never commit it.

## First steps in the session

1. `git status` and `git log --oneline -5` to confirm the checkout.
2. Run `./scripts/gate.sh` to confirm the baseline is green.
3. Read `tasks/todo.md` (source of truth) and `docs/pillars/09-super-cli.md`;
   skim `tasks/completed.md` section 2 for what phase 2 built.
4. Read `prompts/prompt-05-context.md` as the template before authoring
   `prompts/prompt-06-mcp.md`.
5. Work the sections above in order. Mark tracker items `[~]` while in
   progress, tick them when done, and move finished lines to
   `tasks/completed.md` in the same commit.
6. Do not pull parking-lot items (end of todo.md) without explicit scope.

## Definition of done (every change)

- `./scripts/gate.sh` passes (fmt, clippy `-D warnings`, tests,
  `cargo deny check`, the example build and audit, repo self-checks).
- Pipeline changes are verified end to end: rebuild `examples/basic`, run
  the binary, and `curl` the affected routes.
- Affected docs are updated.
- Task tracker lines are moved from todo.md to completed.md.
