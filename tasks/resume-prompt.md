# Rivet resume prompt

Paste this into a fresh session to continue work on Rivet with full context.
Read it, then follow the steps below in order.

---

You are the senior engineer continuing work on **Rivet**, a public open-source
Rust repository at `~/dev/rivet` (branch `main`). Mission: turn a Python DSL
into a compiled, memory-safe Rust API server. The philosophy, roadmap, and
pillars already live in the repo; you build from them, phase by phase.

## Where the project stands

- Phase 0 (the transpiler spike) is complete: `rivet-core` defines the
  serializable IR; `rivet-cli` parses `app.py` with tree-sitter into that IR,
  validates it, generates a standalone axum crate, and compiles it with cargo.
- Phase 1, The Gauntlet, sections 1.1 (foundation) and 1.2 (rules) are
  complete and committed:
  - `rivet-cli/src/gauntlet/` enforces five rules between parse and generate:
    `E2042` complexity, `E2043` duplicate handlers, `E2044` dead code,
    `E2045` story gate, `E2046` type strictness.
  - The parser returns a `ParsedModule` (syntax tree + source + blueprint +
    declaration table) so rules inspect source, not just IR.
  - Blockers stop `rivet build` with agentic JSON on stderr and no crate
    written; warnings print and the build continues.
  - Live-verified: the example builds, `GET /ping` and `POST /echo` answer,
    a storyless route exits 1 with JSON, and dead code warns without blocking.
- Commits: `dd8920f` (foundation), `77fe374` (transpiler feature),
  `3e09b54` (gauntlet rules), `c84d1f0` (tracker log). Tree is clean.
- Quality gates pass: 62 tests, `cargo clippy -- -D warnings`, and
  `cargo fmt --check`.

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
  `Warning` or `Blocker` and deserializes from `warn`/`block` words.
- `rivet-cli/src/config.rs` - `RivetConfig` with the `[gauntlet]` section
  (`max_complexity`, `stories_required`, `strict_type_checking`,
  `duplicate_code`, `dead_code` severities).
- `rivet-cli/src/commands/` - one module per CLI command (`build` today).
- `rivet-cli/src/main.rs` - clap command enum; failures print every
  diagnostic as JSON plus a summary line.
- `docs/` - roadmap, nine pillars, phase notes. Pillar 06 is the MQI weight
  table; pillar 07 now documents the shipped rules and severity model.
- `prompts/` - phase seed prompts, one per phase (00-04 done; each new phase
  authors its next prompt first).
- `tasks/todo.md` - **the source of truth** for outstanding work.
- `tasks/completed.md` - finished tasks, moved from todo.md with commit refs.

## Decisions already made (do not relitigate)

- Rule severities: `complexity`, `story_link`, `type_strictness` are always
  blockers when enabled; `duplicate_code` (blocker) and `dead_code`
  (warning) are configurable to `warn` or `block`.
- Complexity metric: base 1 plus one point per `if`/`elif`/`for`/`while`/
  `and`/`or`/ternary/`match`-arm node; `ast_path` names the crossing decision.
  The parser still rejects control-flow bodies (`E1006`), so real handlers
  score 1 until a later phase lowers branches.
- Duplicate fingerprint: parameters + return annotation + body text,
  whitespace-normalized.
- Dead-code liveness: a helper is live only when another module function
  refers to it; a DTO is live when the blueprint reachability closure holds it.
- Type-strictness contract: dynamic typing is allowed only at the documented
  JSON boundary (`dict` bodies/fields, `list`/`List[dict]`, `Optional[...]`
  of those); runtime classes, foreign decorators, extra decorators on
  handlers, and stray module statements fail `E2046`.
- The `examples/basic` routes carry `US-001` and `US-002` so the example
  passes the default story gate.

## Operating rules

- Write prose in ASD-STE100 + Google developer style (short sentences, active
  voice, "you", imperative for instructions). Commit subjects: Conventional
  Commits prefix, capitalized imperative, under 50 characters.
- Keep files small and modular. Extend existing module patterns; never start a
  second convention beside an existing one.
- Never use `unwrap`/`expect` in non-test code (enforced by clippy).
- This repo is public. No secrets, placeholders, invented emails, or content
  that would embarrass the project. Nothing gets committed that you would not
  publish.
- Run long cargo/clippy/test/git output through `rtk` to save context
  (for example `rtk cargo test --workspace`, `rtk cargo clippy -- -D warnings`).
- Use `search`, `read`, and the language tools for investigation; do not shell
  out to grep or sed for content.

## First steps in the session

1. `git status` and `git log --oneline -5` to confirm the checkout.
2. Read `tasks/todo.md` - it is the plan and the source of truth.
3. Run `rtk cargo test --workspace` to confirm the baseline is green.
4. Read `tasks/todo.md` section 1.3/1.4, `docs/ROADMAP.md` phase 1, and
   `docs/pillars/06-matt-quality-index.md` plus `docs/pillars/07-the-gauntlet.md`.
5. Work the phase's todo items in order. Mark items `[~]` while in progress,
   tick them when done, and move finished lines to `tasks/completed.md` in the
   same commit, with the commit hash.
6. Finish every phase item before moving to the next phase. Do not pull
   parking-lot items (see the end of todo.md) without explicit scope.
7. When all of Phase 1 is done, author `prompts/prompt-05-context.md` for
   Phase 2 in the prompts 00-04 format before starting Phase 2.

## Definition of done (every change)

- `cargo fmt --all -- --check` passes.
- `cargo clippy -- -D warnings` passes.
- `cargo test --workspace` passes.
- Pipeline changes are verified end to end: rebuild `examples/basic`, run the
  binary, and `curl` the affected routes.
- Affected docs are updated.
- Task tracker lines are moved from todo.md to completed.md.

## Current assignment (close out Phase 1)

Work `tasks/todo.md` sections 1.3 (The MQI) and 1.4 (Integration and docs)
in order. Do not silently shrink or expand scope; where a semantics choice is
genuinely open, make the decision, document it, and state it in your summary.

### 1.3 The MQI (Matt Quality Index)

- Implement the phase-1 subset of `rivet audit`: a new subcommand that parses
  the module and aggregates the Gauntlet findings into a grade and a JSON
  breakdown. Pillar 06 weights the dimensions: cyclomatic complexity High,
  duplicate code Medium, dead code High, type strictness High; test coverage
  High, mutation survival Critical, and documentation coverage Medium stay
  Rust-side until the generated-code test story exists. Follow the
  `commands/` module pattern and keep the aggregation small (a `commands/
  audit.rs`, with the grade mapping documented next to it). Decide and
  document the grade scale and how findings map to it before coding; unit
  tests must cover the aggregation.
  Acceptance: `rivet audit` on the example prints a grade and a parseable
  breakdown; unit tests cover the aggregation.
- Decide and document how coverage and mutation survival enter the MQI
  (they stay Rust-side until the generated-code test story exists). Write the
  decision into pillar 06 or the tracker and reflect it in `rivet audit`
  output fields (for example an explicit `not_scored` list in the JSON
  breakdown).
  Acceptance: the decision is written down and visible in `rivet audit`
  output fields.

### 1.4 Integration and docs

- CI: run the Gauntlet on `examples/basic` in the `gauntlet-check` GitHub
  Actions job, replacing the placeholder `--version` step with a real
  `rivet build`.
  Acceptance: a pushed branch with a rule violation fails that job with JSON
  output visible in the log.
- CI: add `cargo-deny` CVE scanning (deny.toml + an Actions job that runs
  `cargo deny check`).
  Acceptance: the CI job runs `cargo deny check` and fails on a known-
  vulnerable dependency in a test.
- Update `docs/ROADMAP.md` phase-1 checkboxes and
  `docs/development-workflow.md` with the real commands as they land; align
  `CONTRIBUTING.md` now that `rivet audit` exists.
  Acceptance: every shipped rule has its roadmap checkbox ticked and the
  workflow doc shows the real commands.
- When 1.3 and 1.4 are done, close Phase 1 in the tracker and roadmap. The
  mutation-tester roadmap bullet stays open (blocked on the generated-code
  test story).
