# Prompt 05: The Context Engine (Persistent State)

**Objective**: Persist Rivet state for humans and AI agents. Record every
CLI command in a local `.rivet/` store, save and resume compact session
context as markdown, and index parsed blueprints in a vector store so
`rivet explain "<symptom>"` can trace a bug to the commit that introduced
it.

**Context**: Phase 1 closed the compile-time story: `rivet build` parses
the DSL, runs the Verifier rules between parse and generate, and compiles
the crate; `rivet audit` grades the module. The parser now returns a
`ParsedModule` with the blueprint the generator consumes, and every command
reports through the agentic-JSON `Diagnostic` path. This phase layers a
local, queryable memory under those commands. It serves
`docs/pillars/04-persistent-context-engine.md` and keeps the core
WASM-friendly per `docs/pillars/data-structured-under-the-hood.md` (SQLite
and vector access live in the CLI, never in `rivet-core`).

---

## Tasks

### 1. Design the store

Define the `.rivet/` store layout and its SQLite schema, then record the
decision in `docs/pillars/04-persistent-context-engine.md`:

- `.rivet/` sits next to the app module (the same directory that holds
  `rivet.toml` and `app.py`); it is gitignored already.
- Choose the SQLite crate for `rivet-cli` and say why. Prefer a maintained
  crate that compiles on macOS, Linux, and (later) WASM CI targets; check
  `rusqlite` with the bundled feature first and record the alternative if
  it does not fit.
- Tables at minimum:
  - `commands`: every CLI invocation with timestamp, exit status, and
    duration
  - `sessions`: saved context blobs with a name and created-at time
  - `fingerprints`: per-commit AST digests for `rivet explain`
- A schema migration must run clean on an empty store and stay idempotent.
- Acceptance: the migration runs clean on an empty store and the crate
  choice is recorded in the pillar doc.

### 2. `rivet history`

Record each command before it runs; `rivet history` lists them in order
with exit status and timing.

- Acceptance: two recorded builds appear in order with correct statuses.

### 3. `rivet session save`

Dump the current command context (the parsed module summary, the config
that ran, and the diagnostics it produced) to compact markdown in the
store.

- Acceptance: the saved session reads back as markdown and round-trips.

### 4. `rivet session resume`

Restore a saved session and print the same compact markdown context so a
human or agent can continue where the session stopped.

- Acceptance: resuming prints the context the save produced.

### 5. Index blueprints

Stand LanceDB up behind a small abstraction and index parsed blueprints:
route paths, DTO shapes, and story IDs become searchable chunks.

- Acceptance: an index test searches a known chunk and ranks it first.

### 6. `rivet explain "<symptom>"`

Combine vector search over the blueprint index, the per-commit AST
fingerprints, and git history to report the commit most likely to have
introduced a symptom.

- Acceptance: the command on a fixture returns the introducing commit with
  a human-readable summary.

### 7. Docs and tracker

Update `docs/pillars/04-persistent-context-engine.md` and the
`docs/ROADMAP.md` phase-2 checkboxes as the features land, and move each
finished tracker line to `tasks/completed.md` in the commit that finishes
it.

Acceptance Criteria
- `rivet history` shows two recorded builds in order with correct statuses.
- `rivet session save` output round-trips through `rivet session resume`.
- An index test searches a known chunk and ranks it first.
- `rivet explain` on a fixture reports the introducing commit with a
  readable summary.
- The `.rivet/` schema migration runs clean on an empty store.
- The SQLite and LanceDB choices are recorded in
  `docs/pillars/04-persistent-context-engine.md`.
- `cargo fmt --all -- --check`, `cargo clippy -- -D warnings`, and
  `cargo test --workspace` all pass.

Agent Instructions
1. Extend the `commands/` module pattern: one small module per new
   subcommand (`history.rs`, `session.rs`, `explain.rs`), each returning
   structured `Diagnostic`s on failure.
2. Keep the store access behind one module (for example
   `rivet-cli/src/store/`) so the schema lives in one place.
3. Never put SQLite or vector code in `rivet-core`; the core stays
   WASM-friendly.
4. Keep modules under 400 lines including tests; no `unwrap` or `expect`
   outside tests (clippy enforces this).
5. Reuse the parser's `ParsedModule` and the agentic-JSON `Diagnostic`
   shape instead of inventing a second convention.
6. Move each finished tracker line to `tasks/completed.md` in the commit
   that finishes it, with the commit hash.

Output
A PR where Rivet remembers: command history, resumable session context,
and a blueprint vector index answer `rivet explain` with the offending
commit — all recorded in pillar 04 and the roadmap.
