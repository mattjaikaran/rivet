# Rivet resume prompt

Paste this into a fresh session to continue work on Rivet with full context.
Read it, then follow the steps below in order.

---

You are the senior engineer continuing work on **Rivet**, a public open-source
Rust repository at `~/dev/rivet` (branch `main`). Mission: turn a Python DSL
into a compiled, memory-safe Rust API server. The philosophy, roadmap, and
pillars already live in the repo; you build from them, phase by phase.

## Where the project stands

- Phase 0 (the transpiler spike) is complete and committed:
  - `rivet-core` defines the serializable IR (routes, DTOs, types,
    handler expressions).
  - `rivet-cli` parses `app.py` with tree-sitter into that IR, validates it,
    generates a standalone axum crate, and compiles it with cargo.
  - Live-verified: `rivet build examples/basic/app.py` produces a binary where
    `GET /ping` returns `{"status":"pong"}` and `POST /echo` echoes the body.
- Commits: `dd8920f` (foundation/hygiene), `77fe374` (transpiler feature).
- All quality gates pass: 30 tests, `cargo clippy -- -D warnings`, and
  `cargo fmt --check`.

## Repo map

- `rivet-core/src/ir.rs` - the IR contract; change it only with care.
- `rivet-cli/src/parser/` - small modular front-end files (`decorator`,
  `signature`, `body`, `expr`, `types`, `validate`).
- `rivet-cli/src/transpiler/rust.rs` - axum code generator.
- `rivet-cli/src/commands/build.rs` - pipeline orchestration.
- `rivet-cli/src/diagnostic.rs` - structured JSON diagnostics.
- `docs/` - architecture, roadmap, nine pillars, phase-0 spike notes.
- `prompts/` - the phase-by-phase build prompts that scaffolded the repo
  (00-03 are finished; each new phase authors its next prompt first).
- `tasks/todo.md` - **the source of truth** for outstanding work.
- `tasks/completed.md` - finished tasks, moved from todo.md with commit refs.

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
4. Read `docs/ROADMAP.md` phase 1 and the pillar docs for the phase you start
   (`docs/pillars/07-the-gauntlet.md`, `docs/pillars/06-matt-quality-index.md`,
   `docs/pillars/05-story-to-code-traceability.md`).
5. Author the phase's seed prompt in `prompts/` (next up:
   `prompts/prompt-04-gauntlet.md`) in the same format as prompts 00-03.
6. Work the phase's todo items in order. Mark items `[~]` while in progress,
   tick them when done, and move finished lines to `tasks/completed.md` in the
   same commit, with the commit hash.
7. Finish every phase item before moving to the next phase. Do not pull
   parking-lot items (see the end of todo.md) without explicit scope.

## Definition of done (every change)

- `cargo fmt --all -- --check` passes.
- `cargo clippy -- -D warnings` passes.
- `cargo test --workspace` passes.
- Pipeline changes are verified end to end: rebuild `examples/basic`, run the
  binary, and `curl` the affected routes.
- Affected docs are updated.
- Task tracker lines are moved from todo.md to completed.md.

## Current assignment (first session after phase 0)

Begin **Phase 1, The Gauntlet**, following `tasks/todo.md` section 1 in order:
author `prompts/prompt-04-gauntlet.md`, define the `rivet-cli/src/gauntlet/`
module layout and severity model, wire the Gauntlet between parse and generate,
then implement the complexity walker with agentic JSON output, followed by the
duplicate-code, dead-code, story-link, and type-strictness rules. Do not
silently shrink or expand scope; if a rule's semantics are genuinely
under-specified, state the decision you make and why in your summary.
