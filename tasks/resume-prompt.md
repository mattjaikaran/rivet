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
- Phase 1 (the Gauntlet) is **complete except the mutation tester**, which
  stays open until the generated-code test story exists:
  - `rivet-cli/src/gauntlet/` enforces five rules between parse and generate:
    `E2042` complexity, `E2043` duplicate handlers, `E2044` dead code,
    `E2045` story gate, `E2046` type strictness.
  - `rivet audit` parses a module, runs the same rules, and folds the findings
    into an A+ to F MQI grade plus a JSON breakdown
    (`commands/audit.rs`; `--json` for the machine shape). Coverage, mutation
    survival, and doc coverage stay Rust-side and appear in the breakdown's
    `not_scored` list.
  - CI runs a real Gauntlet check on `examples/basic` (`rivet build` +
    `rivet audit --json`) and a cargo-deny CVE scan (`deny.toml`).
  - `prompts/prompt-05-context.md` seeds phase 2.
- Quality gates pass: 73 tests, `cargo clippy -- -D warnings`, and
  `cargo fmt --check`. The tree is clean and pushed to `origin/main`.
- Recent commits: `55e599d` (audit), `b0c96c8` (MQI decision),
  `ce70633` (CI gauntlet + cargo-deny), `99f086e` (docs alignment),
  `24be798` (phase-2 prompt), `284711b` (phase-1 tracker close).

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
- `rivet-cli/src/commands/` - one module per CLI command (`build`, `audit`;
  later `history`, `session`, `explain`). Each returns structured
  `Diagnostic`s on failure.
- `rivet-cli/src/main.rs` - clap command enum; failures print every
  diagnostic as JSON plus a summary line.
- `deny.toml` + `.github/workflows/ci.yml` - dependency CVE scanning and the
  three CI jobs (`rust-checks`, `cargo-deny`, `gauntlet-check`).
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

## Next up, in order

### 1. Constraint tools: gate the Rivet repo itself

The repo enforces quality on DSL apps through the Gauntlet, but nothing yet
checks the Rivet source tree with the same rigor. Mirror the constraint-tools
pattern from the Django-ninja-boilerplate reference
(`~/dev/django-ninja-boilerplate/docs/CONSTRAINT_TOOLS.md`, after Uncle Bob
Martin's SwarmForge philosophy): small deterministic programs that produce a
binary pass/fail, chained in one gate, self-enforcing.

Draft the checklist into `tasks/todo.md` under a dedicated section before
coding, then build:

1. **Repository self-checks**: deterministic checks over the source tree —
   file-length ceilings (constraint tools stay under 400 lines; Gauntlet rule
   modules stay under 300 including tests), one documented module per rule,
   error codes listed in the Gauntlet module table stay in sync with the
   rules that emit them, and the tracker (todo/completed) stays coherent.
   Decide and document each threshold so every existing file passes.
2. **One gate command**: an orchestrator that runs fmt, clippy
   (`-D warnings`), tests, `cargo deny check`, the example build and audit,
   and the self-checks, and exits non-zero with a clear message on the first
   failure.
3. **CI wiring**: the orchestrator (or the new self-checks) runs in CI; a
   pushed branch with a planted violation fails the job with the violation
   visible in the log.
4. **Self-enforcement and docs**: each tool passes its own checks; record the
   suite (real commands, thresholds, how to add a check) in
   `docs/development-workflow.md` and `CONTRIBUTING.md`.

Acceptance: one command passes on a clean tree; a planted violation (an
over-long module, an undocumented rule module, a tracker drift) fails it;
CI runs the suite; docs show the real commands.

### 2. Phase 2: the context engine

Author nothing new — `prompts/prompt-05-context.md` is the seed. Draft its
checklist into the phase-2 section of `tasks/todo.md` (items 2.1-2.3 already
exist), then work it in order: the `.rivet/` SQLite store and schema
migration, `rivet history`, `rivet session save`/`resume`, the LanceDB
blueprint index, and `rivet explain "<symptom>"`. Update pillar 04 and the
ROADMAP checkboxes as each lands.

### 3. Coherence as you go

- Keep files small and modular; extend existing module patterns.
- Every behavior change is verified end to end on `examples/basic`
  (rebuild, run, `curl` the affected routes) before you commit it.
- Update pillar docs, ROADMAP checkboxes, and the README in the commit that
  lands the feature.
- Move each finished tracker line to `tasks/completed.md` in the commit that
  finishes it, with the commit hash.
- After 1 and 2 land, author `prompts/prompt-06-mcp.md` for phase 3 before
  starting it.

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
2. Run `rtk cargo test --workspace` to confirm the baseline is green.
3. Read `tasks/todo.md` (source of truth), `prompts/prompt-05-context.md`,
   and `docs/pillars/04-persistent-context-engine.md`.
4. Read the constraint-tools reference at
   `~/dev/django-ninja-boilerplate/docs/CONSTRAINT_TOOLS.md`.
5. Work the sections above in order. Mark tracker items `[~]` while in
   progress, tick them when done, and move finished lines to
   `tasks/completed.md` in the same commit.
6. Do not pull parking-lot items (end of todo.md) without explicit scope.

## Definition of done (every change)

- `cargo fmt --all -- --check` passes.
- `cargo clippy -- -D warnings` passes.
- `cargo test --workspace` passes.
- `cargo deny check` passes.
- Pipeline changes are verified end to end: rebuild `examples/basic`, run
  the binary, and `curl` the affected routes.
- Affected docs are updated.
- Task tracker lines are moved from todo.md to completed.md.
