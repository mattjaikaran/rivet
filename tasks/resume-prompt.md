# Rivet resume prompt

Paste this into a fresh session to continue work on Rivet with full context.
Read it, then follow the steps below in order.

---

You are the senior engineer continuing work on **Rivet**, a public open-source
Rust repository at `~/dev/rivet` (branch `main`). Mission: turn a Python DSL
into a compiled, memory-safe Rust API server. The philosophy, roadmap, and
pillars already live in the repo; you build from them, phase by phase.

## Scope: finish the Python front end first

Rivet is a polyglot transpiler (`docs/ARCHITECTURE.md`). It ingests a
Python/TypeScript DSL and emits Rust. **Only the Python front end exists
today.** Complete all Python-side work before you start any TypeScript work.

In scope now:

- The Python DSL front end and everything that completes it.
- Phase 5's Python-side work: the WASM target (landed), the native
  (Kotlin/Swift) bindings, and the `rust_native_features` flags.

Deferred until the Python work is done:

- A **TypeScript DSL front end**. `docs/ARCHITECTURE.md` and
  `prompts/prompt-01-ir.md` name it as a co-equal parser over the same IR.
  Do not start it, and do not shape the IR for it, without explicit scope.
- The **UniFFI TypeScript (React Native) binding** in phase 5. Build the
  Kotlin and Swift bindings; leave the TypeScript one.

Note: the phase-4 admin panel is **not** TypeScript work. The phase-4 seed
already decided the panel ships as one static file, with no React or Solid
build step (`prompts/prompt-07-ecosystem.md`). Keep it that way.

If a request would pull TypeScript work into the critical path, say so and
finish the Python-side item first.

## Where the project stands

- Phases 0-3 are complete and pushed.
- **Phase 4 is complete** (closed 2026-09-10). All seven deliverables landed:
  the compile-time plugin system (`53242fd`), the multi-service transport
  switch (`a675668`), the polyglot `rivet dev` proxy (`44345fd`), the
  embedded static assets (`7a3646f`), the admin panel (`0851b0d`), service
  discovery for Consul and etcd (`857060d`), and `rivet sync` for Jira and
  Linear (`7605413`, `c5e79d5`). Pillars 01, 02, 03, and 05 describe what
  shipped; the ROADMAP phase-4 boxes are ticked.
- **Phase 5 has started.** The phase seed is
  `prompts/prompt-08-wasm-mobile.md`, and its checklist is in the phase-5
  section of `tasks/todo.md`. Two deliverables landed:
  - **WASM target** (`ddd4d45`, `f0654e5`): `rivet build --target wasm`
    renders the blueprint as a WASI command module and compiles it for
    `wasm32-wasip1`. The crate carries the native target's own DTO structs
    and `mod service`; it has no axum, tokio, gRPC, plugins, or assets, and
    its manifest depends on serde and serde_json alone. It reads one request
    as JSON on stdin, dispatches through a build-time `match`, and writes one
    envelope on stdout. Verified under Wasmtime.
  - **`const_generics`** (`927fb68`): `List[T, N]` renders `[T; N]`, with a
    generated serde bridge because the derive stops at 32 elements. The
    example sets the flag and round-trips 768 values on both targets.
- Remaining phase-5 work:
  - The other three `rust_native_features` flags. Each stays `false` because
    its feature needs a layer the DSL does not have: no database surface
    (`raii_connections`), no route-protection decorator
    (`compile_time_rbac`), and `FieldDefinition::is_borrowed` is never set by
    the parser (`zero_copy_deserialization`). A flag set without its feature
    is now `E2013`.
  - The UniFFI Kotlin/Swift bindings, `rivet mobile init`, and the React
    Native binding (deferred). **These are blocked on toolchains**: no JDK,
    no `kotlinc`, no Android SDK, no `uniffi-bindgen`, and no Xcode
    command-line tools. Record this rather than generating code no toolchain
    here has compiled.
- GitHub Actions auto-runs are paused until the app ships (the workflow is
  `workflow_dispatch`-only). `./scripts/gate.sh` is the acceptance bar:
  fmt, clippy `-D warnings`, **282 tests**, `cargo deny`, the example
  build/audit, and the repo self-checks.
- Tracker coherent: 6 todo items, 129 completed.
- Recent commits: `b185a3e` (tracker hash), `927fb68` (const generics),
  `f0654e5` (wasm fixes and etcd lease test), `ddd4d45` (WASM target),
  `ec45c3d` (phase-4 close), `c5e79d5` (sync hardening), `7605413`
  (`rivet sync`), `857060d` (service discovery), `0851b0d` (admin panel).

### Environment (check before you rely on it)

- `wasm32-wasip1` and `wasmtime` (Homebrew) are installed. Both are needed
  for the WASM acceptance; the module's real platform check reports a
  missing host instead of passing silently.
- Mobile toolchains are absent. `swiftc` and Xcode exist; `java` and
  `kotlinc` do not.

## Disk hygiene (do this)

`rivet build` writes a generated crate with its own cargo `target/` (about
140 MB per app), and `rivet build --target wasm` writes a second one beside
it. `./scripts/gate.sh` regenerates `examples/basic/generated`. The workspace
`target/` grows to about 12 GiB. Clean up when you finish a work session or
before you push:

```bash
make clean        # examples/*/generated, examples/*/generated-wasm, temp fixtures
make clean-all    # the above plus `cargo clean` (about 12 GiB)
```

The cargo cache is warm, so `./scripts/gate.sh` takes about 100 seconds.
After a `make clean-all` the first release build costs about 7 minutes; the
WASM build adds a small target-specific compile. Prefer `target/debug/rivet`
while you iterate.

Tests clean up after themselves: `rivet-cli/src/test_support.rs` defines a
`ScratchDir` guard that removes each fixture on drop (success and panic). Do
not reintroduce pid-suffixed temp paths or hand-rolled `remove_dir_all`.

Never commit build output. `.gitignore` covers `target/`, `generated/`, and
`generated-wasm/`; verify with `git status --short` before every commit. The
example fixture build under `examples/basic/dist/` is committed on purpose:
the asset test renames it.

## Repo map

- `rivet-core/src/ir.rs` - the IR contract; change it only with care.
- `rivet-cli/src/config.rs` - `rivet.toml` (plus `config/tests.rs`):
  `[gauntlet]`, `[plugins]`, `[transport]`, `[frontend]`, `[admin]`,
  `[discovery]`, `[rust_native_features]`.
- `rivet-cli/src/parser/` - modular front end (`decorator`, `signature`,
  `body`, `expr`, `types`, `validate`, `python`).
- `rivet-cli/src/gauntlet/` - the quality rules: `mod.rs` holds the `Rule`
  trait, `Finding`, `run_gauntlet`, and the severity policy.
- `rivet-cli/src/diagnostic.rs` - structured JSON diagnostics.
  `suggested_fix` is a required `String`; do not make it optional again.
- `rivet-cli/src/store/` - `.rivet/` SQLite store plus `vector.rs`.
- `rivet-cli/src/commands/` - one module per command (`build`, `audit`,
  `history`, `session`, `explain`, `fix`, `trace`, `plan`, `add`, `dev`,
  `sync`). Big modules put tests in a sibling `tests.rs` and split
  submodules before a file reaches the 400-line ceiling:
  `build/wasm.rs`, `build/tests/{registry,wasm}.rs`, `sync/issue.rs`,
  `sync/jira.rs`, `sync/linear.rs`, `sync/story.rs`, `sync/config.rs`,
  `sync/http.rs`, `sync/shape.rs`, `discovery/etcd.rs`.
- `rivet-cli/src/transpiler/rust.rs` + `rust/` - the generator:
  `service.rs` (transport-free route logic), `handler.rs` (thin axum
  handlers), `channel.rs`, `assets.rs`, `admin.rs`, `discovery.rs`,
  `main_file.rs` (native assembly and manifest), `helpers.rs` (the runtime
  helpers both targets emit), `wasm.rs` + `wasm/tests.rs` (the WASI target),
  `tests.rs` + `tests/{fixtures,features}.rs`.
- `rivet-cli/src/test_support.rs` - the shared `ScratchDir` test guard.
- `constraint-tools/` + `scripts/gate.sh` - self-checks and the gate.
- `docs/` - roadmap, nine pillars, phase notes. Pillar 08 documents the WASM
  target and the flag; pillar 05 documents `rivet sync`.
- `tasks/todo.md` is **the source of truth**; `tasks/completed.md` holds
  finished lines with commit refs.

## Decisions already made (do not relitigate)

- **Python front end first; no TypeScript work yet.** See the scope section.
- **The WASM target is a WASI command module, not a server.** `wasm32-wasip1`
  with a `fn main` that reads one request on stdin and writes one envelope on
  stdout. `axum::serve` has no path on WASI: `tokio`'s `net` and
  `rt-multi-thread` features do not compile there, and WASI creates no
  socket. Do not "fix" the target by adding them.
- **The wasm crate shares `mod service` and the DTO structs, and nothing
  else.** No plugins, no `[admin]`, no `[discovery]`, no `[frontend]`. The
  build prints a note naming each configured feature the module omits, so a
  capability is never dropped in silence.
- **The request `body` travels as a JSON string** (or a JSON value, which the
  module re-serializes); the response `body` is always a value. Both are
  documented in pillar 08.
- **A `[rust_native_features]` flag is true only when its feature is
  implemented.** A set flag with no implementation is `E2013`, because the
  section is a claim about what the generator does.
- `suggested_fix` is required on `Diagnostic` and `Finding`.
- The generated backend mounts the blueprint's own route paths. `/api` is a
  proxy-level convenience only; do not add an `/api` mount to the generator.
- `[frontend]` owns the production build (`dist`, `spa`), not a separate
  `[assets]` table.
- The embedded single-page fallback stays a *navigation* rule (no file
  extension, `Accept: text/html`).
- The admin panel ships as one static file; do not add a React or Solid
  build.
- `rivet sync` binds an issue to a story through the issue title's prefix
  before the first colon, compared to the story ID exactly. Only the orphan
  check uses a shape heuristic. Never write to the tracker without `--apply`.
- Rule severities, the complexity metric, duplicate fingerprinting, dead-code
  liveness, the type-strictness contract, and the MQI grade scale are
  unchanged from phases 1-2.
- CI auto-runs stay paused until the app ships; local `./scripts/gate.sh` is
  the bar.

## Next up, in order

1. **The remaining three `rust_native_features` flags.** Each needs its layer
   first, and the order is the order of the layers:
   - `zero_copy_deserialization`: teach the parser to set
     `FieldDefinition::is_borrowed` and render a borrowed `&str` with a
     lifetime; then flip the flag. Acceptance: a DTO with a borrowed `str`
     field builds and a request round-trips without owned copies.
   - `raii_connections`: needs a database surface in the DSL, which does not
     exist. Treat as blocked until a database story is in scope.
   - `compile_time_rbac`: needs a way to mark a route protected. The
     decorator accepts only `path` and `stories` today.
2. **The phase-5 WASM follow-ups, if any land from review**: the module's
   integration fixture now covers a fixed-size array and both body shapes.
   Keep the module's real-platform check (`wasmtime run`) green.
3. **The mobile deliverables** (blocked): UniFFI bindings for Kotlin and
   Swift, then `rivet mobile init --platforms ios,android`. These need a JDK,
   `kotlinc`, the Android SDK, `uniffi-bindgen`, and Xcode's command-line
   tools. If the toolchains appear, build the bindings over `mod service` so
   the binding surface and the HTTP surface cannot drift.
4. **Phase-5 docs and tracker close** once the phase's reachable work is
   done and the mobile lines are either landed or recorded as blocked.

## How to work (house rules)

- **Use subagents.** Fan out with the `task` tool for parallel, file-disjoint
  work: give each subagent a role, explicit file paths, exact acceptance
  criteria, and a `local://` spec when the change is large. A read-only
  question about unfamiliar code goes to a `scout` subagent. Never let a
  subagent make a design decision, and never let one run the gate. If a spawn
  fails with `No model selected`, that is an environment fault, not a task
  fault: work the slices inline and keep the same file discipline.
- **Save context with `rtk`.** Route long output through it:
  `rtk cargo test --workspace`, `rtk cargo clippy -- -D warnings`,
  `rtk ./scripts/gate.sh`, `rtk git log`. Use surgical reads
  (`read` with `offset`/`limit`) and never re-read what you hold.
- **Commit in small, coherent units, and commit when you finish one.** One
  commit per unit (feature, docs, tracker move), Conventional Commits prefix,
  capitalized imperative subject, 50 characters or fewer, body wrapped at 72.
  Commit the feature first, then the tracker move, so the hash exists when
  the tracker line records it. Never commit a literal placeholder such as
  `<pending>` while staging the feature: fill the hash in the follow-up
  tracker commit.
- **Verify before you commit.** Run the specific test or smoke test that
  covers the change; run `./scripts/gate.sh` once over the union of the
  session's changes, not per file. For the WASM target, run the module under
  `wasmtime` — a unit test on generated text is not proof.
- **Clean up generated output before you stop** (`make clean`, or
  `make clean-all` when disk is tight). Never leave `target/`,
  `examples/*/generated/`, or `examples/*/generated-wasm/` in a commit.
- **Reject bad code, not just failing tests.** No stubs, placeholders,
  TODO-shims, speculative abstractions, duplicated logic, dead code, or
  invented facts. Fix at the source instead of papering over the symptom.
  A generator bug that surfaces as cargo's `E2009` is a generator bug: give
  it its own diagnostic (`E2011`-`E2013` are the newest examples).
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

## First steps in the session

1. `git status` and `rtk git log --oneline -10` to confirm the checkout.
2. `rtk ./scripts/gate.sh` to confirm the baseline is green.
3. Read `tasks/todo.md` (source of truth), `docs/ROADMAP.md` phase 5, and
   `prompts/prompt-08-wasm-mobile.md`.
4. Check the environment before planning phase-5 work:
   `rustup target list --installed | grep wasip1`, `wasmtime --version`,
   `which kotlinc java`.
5. Start at "Next up" item 1. Decompose it into subagent-sized,
   file-disjoint tasks.
6. Mark tracker items `[~]` while in progress, then move the finished line to
   `tasks/completed.md` with its commit hash in a follow-up commit.
   `check-tracker` rejects any `[x]` left in `todo.md`, so never tick a line
   in place.
7. Do not pull parking-lot items (end of `tasks/todo.md`) without explicit
   scope. A TypeScript front end is one of them; leave it.

## Definition of done (every change)

- `./scripts/gate.sh` passes (fmt, clippy `-D warnings`, tests,
  `cargo deny check`, the example build and audit, repo self-checks).
- Pipeline changes are verified end to end: rebuild `examples/basic`, run the
  binary, and `curl` the affected routes. WASM changes also run the module
  under `wasmtime`.
- Affected docs are updated (pillar doc, ROADMAP boxes, README).
- Task tracker lines are moved from `todo.md` to `completed.md`.
- `make clean` has been run if the session generated large build output.
