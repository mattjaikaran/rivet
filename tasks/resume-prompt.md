# Rivet resume prompt

Paste this into a fresh session to continue work on Rivet with full context.
Read it, then follow the steps below in order.

---

You are the senior engineer continuing work on **Rivet**, a public open-source
Rust repository at `~/dev/rivet` (branch `main`). Mission: turn a Python DSL
into a compiled, memory-safe Rust API server, and publish it.

**This session has one priority: close a correctness defect class before any
roadmap work.** A previous session shipped features whose defects were found
by probing, not by the test suite. The generator accepts DSL input that
parses, passes every gate, and then produces a Rust crate that does not
compile — and the native and WASM targets sometimes disagree about the same
blueprint, which is worse. Fix the class, add the guard that would have caught
it, and only then continue the roadmap.

**Two facts in this document are unverified and one is a correction to an
earlier plan.** They are flagged `VERIFY FIRST`. Do not build on them until
you have run the check — this document has been wrong before, in both
directions, and a plan built on a wrong premise wastes a whole session.

## Scope: finish the Python front end first

Rivet ingests a Python/TypeScript DSL and emits Rust (`docs/ARCHITECTURE.md`).
**Only the Python front end exists today.** Complete all Python-side work
before you start any TypeScript work.

In scope: the Python front end, phase 5's Python side (the WASM target —
landed — plus the native Kotlin/Swift bindings and the `rust_native_features`
flags), and the hardening work in this prompt.

Deferred: a **TypeScript DSL front end**, and the **UniFFI TypeScript (React
Native) binding**. Do not start either, and do not shape the IR for them,
without explicit scope.

The phase-4 admin panel is **not** TypeScript work: it ships as one static
file with no React or Solid build step. Keep it that way.

## The defect class

### Root cause

The generated crate is **one flat Rust namespace** for user-derived
identifiers (handler function names, DTO struct names) and generator-owned
identifiers (runtime helpers, modules, imported types). Nothing reserved the
generator's names, so a user who picks one gets cargo errors against generated
code instead of a Rivet diagnostic.

A second, worse symptom: **the two targets disagree.** The native generator
emits handlers at the top level; the WASM generator emits them inside a
module. So the same blueprint gets two different answers.

### Confirmed cases (verified by running them at `27620d2`)

Reproduce by writing the snippet to `app.py` beside a `rivet.toml` holding
only `[project] name = "p"`, then `rivet build [--target wasm] app.py`.

| Input | Native | WASM |
| :--- | :--- | :--- |
| handler named `main` | cargo fail | builds |
| handler named `json_obj` | cargo fail | builds |
| handler named `json_number` | cargo fail | builds |
| handler named `channel_error` | cargo fail | builds |
| DTO `String` | cargo fail | cargo fail |
| DTO `Vec` | cargo fail | cargo fail |
| DTO `Json` | cargo fail | builds |
| DTO `service` | cargo fail | cargo fail |
| DTO `serde_json` | cargo fail | cargo fail |

`cargo fail` means `rivet build` reported progress, ran cargo, and cargo
reported errors — typically `E0428` (defined multiple times) or a name that
resolves to the user's DTO instead of the intended type.

**Also broken, established by reading `main_file.rs:157`:** that line emits
one `use axum::{extract::{Json, State}, routing::{...}, Router};` at crate
root. So **DTO `State` and DTO `Router` fail exactly like DTO `Json`.** They
are missing from the table above only because nobody probed them. Confirm
with a run, then add them to the reserved set **and** to the class test.

### VERIFY FIRST — handler names and the reserved set

An earlier plan said: reject any handler name or DTO name that is in the
reserved set. **That is probably wrong for handler names**, and the class test
must not pin a wrong contract.

A handler becomes a Rust **function** (value namespace). A DTO becomes a Rust
**struct** (type namespace). `mod service` and `use axum::Router` occupy the
type namespace. So:

- a **DTO** named `service`/`String`/`State`/`Router` collides → must be
  rejected;
- a **handler** named `service`/`String`/`State`/`Router` very likely does
  **not** collide: after Layer 1 it lives inside `mod handlers`, where a local
  `fn service` sits in the value namespace and silently shadows nothing that
  matters. Rust glob imports (`use super::*`) have lower precedence than a
  local item, so this is not even an error.

**Run this probe before you write the check:**

```bash
# one case per target: does a handler with a reserved name still fail cargo?
mkdir -p /tmp/vf && printf 'from rivet import api\n\n@api.get("/m", stories=["US-1"])\ndef service() -> dict:\n    return {}\n' > /tmp/vf/app.py
printf '[project]\nname = "p"\n' > /tmp/vf/rivet.toml
./target/release/rivet build /tmp/vf/app.py
```

Repeat for `String`, `State`, `Router`, and a handler named `rivet_x`.

Then scope the check to what the probe shows:

- **handler names**: reject the `rivet_`/`RIVET_` prefix **only**, unless the
  probe disproves it;
- **DTO names**: reject the full reserved set plus the prefix;
- **DTO field names**: reject **nothing** from the reserved set. A field lives
  inside its struct (`pub main: String` inside `pub struct Foo` compiles), so
  including fields would refuse working input. The keyword check `E1011`
  already covers `type: str`.

If the probe contradicts this, follow the probe and say so in the commit body.

### The fix is one invariant, not a pile of patches

> Every name the generated crate emits belongs to the generator. A DSL
> identifier that would collide with one is rejected, with a line number,
> before any crate is written — and both targets agree on every input.

Three layers. Make collisions structurally impossible where you can; validate
the small remainder where you cannot.

**Layer 1 — namespace the handlers.** Emit native handlers inside
`mod handlers` (the WASM target already does this) and register them as
`handlers::ping::<channel::InProcess>`. This kills every handler-versus-helper
collision at once and makes the targets symmetric. It does not replace
`E1012`: two routes sharing a handler name are still rejected, because a
module cannot hold one name twice.

**Layer 2 — prefix everything the generator owns.** Rename runtime helpers and
internal modules with a reserved prefix: `rivet_json_obj`,
`rivet_json_number`, `rivet_channel_error`, `rivet_to_json`,
`rivet_dispatch`, `rivet_body_text`, `rivet_answer`, `rivet_json_error`,
`rivet_allowed_methods`, `RIVET_DECLARED_PATHS`, `mod rivet_executor`,
`mod rivet_fixed_array`. Keep the *public* generated surface readable —
`mod service`, `mod channel`, `mod admin`, and the DTO structs stay, because
the pillars document them.

**Layer 3 — one guard for the residual set.** Add **`E1013`** in
`rivet-cli/src/parser/python.rs`, so it carries a real file and line and every
command rejects the module (`build`, `audit`, `trace`, `plan`, `mcp`), not
only the two generators. Scope it per the probe above.

### Derive the reserved set, so it cannot rot

A hand-maintained denylist rots when someone adds a helper. Give the list and
the emission one source of truth: **`rivet-core/src/reserved.rs`** exports the
reserved names *and* the constants the generator emits, so a rename cannot
drift from the check.

```rust
/// The module the generated handlers live in.
pub const HANDLERS_MODULE: &str = "handlers";
/// The prefix every generator-owned symbol carries.
pub const PREFIX: &str = "rivet_";

/// Names the generated code writes at crate root. A DTO may not reuse one.
pub const RESERVED: &[&str] = &[
    // Generated modules (pillar 02 documents service and channel).
    "service", "channel", "admin", "assets", "discovery", HANDLERS_MODULE,
    // The crate root already holds this.
    "main",
    // Types the `use axum::…` line and the emitter write by name.
    "Json", "State", "Router", "String", "Vec", "Option", "bool", "i64", "f64",
    // Paths the generated code writes by name.
    "serde", "serde_json", "axum", "tokio", "tracing", "std",
];
```

`Option` is a judgment call: it compiles today only while no field anywhere is
`Optional[...]`, because the emitter writes `Option<{base}>` only for an
optional field. That makes the rule depend on an unrelated field, so reserve
it unconditionally and say why in the comment. Predictable beats permissive.

Then:

- the generators emit `reserved::HANDLERS_MODULE` rather than a literal;
- the parser checks the set plus `PREFIX` for **DTO names**, and `PREFIX`
  alone for **handler names** (pending the probe);
- a unit test in `reserved.rs` asserts that `RESERVED` contains every module
  name the generator constants define, so adding a module without reserving it
  fails a test instead of shipping.

### The class test — the guard that was missing

Add a table-driven integration test at
`rivet-cli/src/commands/build/tests/collisions.rs`, registered in
`commands/build/tests.rs`.

**Each row carries its expected outcome**, because the fix changes what is
legal. Layers 1 and 2 make several current failures *build*; a test asserting
a diagnostic for every row would fail against its own fix.

| Input | Expected | Why |
| :--- | :--- | :--- |
| handler `main`, `json_obj`, `json_number`, `channel_error` | builds | `mod handlers` + the prefix |
| DTO `fixed_array`, `executor` | builds | those modules are `rivet_*` now |
| handler `rivet_*` | `E1013` | the prefix is reserved |
| DTO `rivet_*` | `E1013` | the prefix is reserved |
| DTO `String`, `Vec`, `Option`, `Json`, `bool`, `i64`, `f64` | `E1013` | the emitter writes those types |
| DTO `State`, `Router` | `E1013` | they are in the `use axum::…` line |
| DTO `service`, `channel`, `admin`, `assets`, `discovery`, `handlers` | `E1013` | the generated modules own those names |
| DTO `serde`, `serde_json`, `axum`, `tokio`, `tracing`, `std` | `E1013` | the generated code writes those paths |
| handler `service`, `String`, `State`, `Router` | **probe decides** | value vs type namespace — do not guess |

Assert per row, on **both targets**:

1. the actual outcome matches the expected one;
2. **native and WASM agree.** This is the invariant, and the assertion that
   matters most — the original defect included the two targets disagreeing,
   and a per-target test would have let that ship.

For every `E1013` row, also assert the diagnostic names the offending
identifier and carries a line. Add no rows for DTO **field** names.

This test is the actual fix. Those cases existed because every generator test
fed the generator a name the *test author* chose; the suite never asked what
happens when the user chooses.

## What is verified working — do not break it

- **Phases 0-4 complete.** Phase 4 closed 2026-09-10: compile-time plugins, the
  multi-service transport switch, `rivet dev`, embedded static assets, the
  admin panel, service discovery (Consul and etcd), and `rivet sync` (Jira and
  Linear).
- **Phase 5 started.** `rivet build --target wasm` emits a `wasm32-wasip1`
  command module sharing the native target's `mod service` and DTO structs,
  carrying no axum/tokio/gRPC/plugins/assets, verified under Wasmtime. The
  `const_generics` flag renders `List[float, 768]` as `[f64; 768]` with a
  generated serde bridge, because the derive stops at 32 elements.
- **Three checks already landed**, and they are the pattern to follow:
  `E2014` (two routes on one method and path — the native router panicked at
  startup, the WASM dispatch silently kept the first arm), `E1012` (two routes
  sharing a handler name — cargo reported "defined multiple times"), and
  `E2013` (a feature flag set without its feature).
- `./scripts/gate.sh` is the acceptance bar: fmt, clippy `-D warnings`, **287
  tests**, `cargo deny`, the example build and audit, repo self-checks. Green
  at `27620d2`.
- Tracker coherent: 6 todo items, 135 completed.

### Environment

- `wasm32-wasip1` and `wasmtime` (Homebrew) are installed; both are needed for
  the WASM acceptance.
- Mobile toolchains are **absent**: no JDK, no `kotlinc`, no Android SDK, no
  `uniffi-bindgen`; `swiftc` and Xcode exist.

## How to work: subagents

The work is genuinely parallel. Decide the contract first, then fan out.

**Contract — settle all of it before spawning, and put it in the batch
`context` so every subagent builds the same shape:**

- the reserved prefix (`rivet_` / `RIVET_`);
- the handler module name (`handlers`) and the constant in
  `rivet-core/src/reserved.rs` that defines it;
- the exact `RESERVED` list, the rule that generators emit `reserved::*`
  constants, and the **scoping** (full set for DTO names, prefix only for
  handler names, nothing for field names);
- the code (`E1013`), raised in the parser, not the generators;
- that **`{service}` stays emitted before `{handlers}`** in the assembled
  `main.rs`. `rivet trace` resolves a route's logic by scanning the generated
  source line by line (it looks for `fn {handler}(`), so moving the handler
  block ahead of the service block silently breaks it. A different order means
  `trace.rs` changes in the same commit.
- that every row must agree across both targets.

**Wave 1 — three file-disjoint slices, one `task` batch:**

1. `NativeNamespacing` — owns
   `rivet-cli/src/transpiler/rust/{main_file,handler,helpers,service}.rs`,
   `rivet-cli/src/commands/trace.rs`, and the call sites that hardcode the
   current registration: `transpiler/rust/tests.rs` (the
   `.route("/ping", get(ping::<channel::InProcess>))` assertion),
   `transpiler/rust/tests/features.rs` (the
   `post(create_ping::<channel::InProcess>)` assertion), and the trace-output
   assertion in `commands/trace.rs`. Acceptance: `examples/basic` builds,
   `rivet trace` still finds the service function, and a handler named
   `json_obj` no longer reaches cargo.
2. `WasmNamespacing` — `transpiler/rust/wasm.rs` and
   `transpiler/rust/wasm/tests.rs`. Acceptance: the module still answers under
   `wasmtime run`.
3. `ReservedNames` — new `rivet-core/src/reserved.rs`, plus
   `parser/python.rs` and `parser/python/tests.rs`. Acceptance: the parser
   rejects `rivet_x` and `String` (a DTO) with `E1013` and a line.

Disjoint files, so they run at once. Slice 3 defines `reserved.rs`, which the
other two consume — give all three the exact constant names in the contract so
they compile against the agreed shape without waiting.

**Wave 2 — after wave 1 lands:** `CollisionClassTest` — new
`commands/build/tests/collisions.rs` plus registration in
`commands/build/tests.rs`. It asserts wave 1's combined output, so it cannot
start earlier.

**Wave 3 — the integration commit:** docs (`docs/pillars/02-*`,
`docs/pillars/08-*`, `README.md` if layout changes), tracker, resume prompt.
Do this yourself, not by subagent.

Rules for every batch:

- **Skip validation in subagents.** Tell each explicitly: do not run
  `./scripts/gate.sh`, `cargo clippy`, or the full suite; do not run
  formatters. Run those once, yourself, over the union.
- Give each a `# Target` / `# Change` / `# Acceptance` task, exact file paths,
  the contract, and an acceptance criterion stated as an observable result.
- Never let a subagent make a design decision. Never let two own one file; if
  they must, serialize them.
- A read-only question about unfamiliar code goes to a `scout` subagent, not a
  chain of reads.
- If a spawn fails with `No model selected`, that is an environment fault, not
  a task fault: work the slices inline and keep the same file discipline.

## How to work: tokens and `rtk`

```bash
rtk ./scripts/gate.sh
rtk cargo test --workspace
rtk cargo clippy -- -D warnings
rtk git log --oneline -20
rtk git diff --stat
```

Use surgical reads (`read` with `offset`/`limit`); never re-read what you hold.
Prefer `target/debug/rivet` while iterating. The cache is warm, so the gate
takes about 100 seconds.

## How to work: commits in a public repository

People will read this history. Each commit must stand on its own.

- **One commit per coherent unit.** Conventional Commits prefix, capitalized
  imperative subject, 50 chars or fewer, no trailing period, blank line, body
  wrapped at 72.
- **The body says what and why, not how.** The diff shows how.
- **A fix commit names the symptom and the cause.** "Reject two routes on one
  method and path" plus a body quoting the panic is good. "Fix bug" is not.
- **Never commit a placeholder.** Do not stage `<pending>` or `TBD` while
  waiting for a hash. Land the feature commit, then fill the real hash in a
  follow-up tracker commit.
- **Never commit build output.** `.gitignore` covers `target/`, `generated/`,
  and `generated-wasm/`. `git status --short` is not enough: use
  `git check-ignore -v <path>`. The one deliberate exception is
  `examples/basic/dist/`, which the asset test renames.
- **Check a diagnostic code is free before you use it.** Read the code table
  (`parser/python.rs` for `E1xxx`, `gauntlet/mod.rs` for rule codes) and grep
  the tree: `grep -rho '"E[0-9]\{4\}"' rivet-cli/src | sort -u`. A previous
  session shipped `E1010` twice, caught in review rather than by the suite.
- **Verify before you commit.** Run the test that covers the change. Run the
  gate once over the union. For WASM, run the module under `wasmtime` — a unit
  test on generated text is not proof.
- **Clean up:** `make clean` (or `make clean-all`). Never leave generated
  crates behind.

## House rules

- **Fix at the source.** No stubs, placeholders, `TODO` shims, speculative
  abstractions, duplicated logic, dead code, or invented facts.
- **A bug that surfaces as cargo's `E2009` is a generator bug.** Give it its
  own diagnostic. The newest are `E1012` (parser, duplicate handler name) and
  `E2011`-`E2014` (generator: array shapes, the optional fixed array, an
  unimplemented flag, overlapping routes).
- **Put a guard where it has the most context.** A check needing a file and
  line goes in the parser, so every command rejects the input. A check about
  rendered output goes in the generator and must be called by *both*
  generators.
- **One pattern per concern.** Match the module layout, error-code ranges
  (`E1xxx` parser, `E2xxx` generator and Gauntlet, `E3xxx` context engine and
  agentic commands), and the diagnostics.
- **Rust best practices.** Idiomatic ownership over clones; small fallible
  functions; `Result` with structured errors; no panics in library paths; no
  `unsafe` without a soundness comment; no `unwrap`/`expect` outside tests;
  build JSON by hand from `serde_json::Value` (`json!` is clippy-banned).
- **File ceiling is 400 lines** (`constraint-tools` enforces it). Split before
  you reach it, into sibling modules with a `tests.rs`.
- **This repo is public.** No secrets, placeholders, or personal content.

## Process failures, and what to change

### Why the suite missed this

**Every generator test fed the generator a name the test author chose.** The
fixtures use `ping`, `echo`, `OrderCreate`, `Embedding`. No test asked what
happens when the *user* chooses, and the Gauntlet cannot see the question —
`def json_obj` is an ordinary Python function. The failure only exists after
generation, so it needs a test that generates and compiles.

Three habits follow, for the rest of the roadmap:

1. **A test that only supplies inputs you picked proves the happy path.** For
   anything rendering user input into another language, add a row per *shape
   of input the user controls*: names, empty values, unicode, boundary
   lengths. The class test is the model.
2. **A green gate is necessary, not sufficient.** It proves the fixtures
   compile; it cannot prove an input outside them does. Spend one command
   probing a new capability's edges — the probes that found this defect took
   roughly two minutes each and found six cases the suite had missed.
3. **Reproduce before you fix, and do not trust a table you were handed.**
   This document was wrong twice in both directions: it claimed a case was
   untested that had just been probed and does fail cargo, and it omitted
   `State`/`Router` entirely. Run the case, then fix it.

### Other defects this project survived

- **`.gitignore` held a bare `build/`**, which also matched
  `rivet-cli/src/commands/build/`. Every integration test there — transport,
  assets, the admin panel, discovery — was untracked, so a fresh clone ran the
  gate without them. Fixed in `4263fc3`.
- **An output directory was nearly committed.**
  `examples/basic/generated-wasm/` arrived with the WASM target and was in
  neither `.gitignore` nor `scripts/clean.sh`; `git check-ignore -v` caught it
  (`ddd4d45`).
- **A diagnostic code shipped twice** (`E1010`). Fixed in `08c8b1a`.
- **A tracker entry cited a hash whose code no longer matched**, because a fix
  landed after the entry. Cite both commits, or the later one (`9d77297`).
- **A plan was written against a self-contradicting test.** An earlier version
  of this prompt required every collision row to answer a diagnostic, while
  its own fix made half those rows legal. Read the class-test table and the
  layer descriptions together before starting; if they conflict, the table is
  right.

## First steps in the session

1. `git status` and `rtk git log --oneline -10` to confirm the checkout.
2. `rtk ./scripts/gate.sh` — expect green, 287 tests, 6 todo, 135 completed.
3. **Run the `VERIFY FIRST` probe** for handler names in the reserved set, and
   separately confirm DTO `State` and DTO `Router` fail cargo. Nothing below is
   safe until you know these.
4. Reproduce two table rows yourself — `handler_json_obj` (native) and
   `dto_Json` (both targets, to see the divergence). Do not take the table on
   trust.
5. Read `transpiler/rust/main_file.rs`, `transpiler/rust/wasm.rs`, and
   `parser/python.rs` far enough to place the three layers.
6. Write the contract (prefix, module name, scoped reserved rule, `E1013`),
   then run wave 1 as one `task` batch with three subagents.
7. Land wave 1, then wave 2. Verify: every table row produces its expected
   outcome on **both** targets, with no unexpected `CARGO-FAIL` and no
   divergence.
8. Update docs and the tracker in the integration commit. Move finished lines
   from `tasks/todo.md` to `tasks/completed.md` with real hashes.
9. Only then continue the roadmap.

## Next up, after the defect class is closed

1. **The remaining three `rust_native_features` flags.** Each needs its layer:
   - `zero_copy_deserialization` — teach the parser to set
     `FieldDefinition::is_borrowed` and render a borrowed `&str` with a
     lifetime; then flip the flag.
   - `raii_connections` — needs a database surface in the DSL. Blocked.
   - `compile_time_rbac` — needs a way to mark a route protected; the
     decorator accepts only `path` and `stories`.
   A flag set without its feature is already `E2013`.
2. **The mobile deliverables**, blocked on toolchains: UniFFI bindings for
   Kotlin and Swift, then `rivet mobile init --platforms ios,android`. If the
   toolchains appear, build them over `mod service` so the binding surface and
   the HTTP surface cannot drift.
3. **Phase-5 docs and tracker close**, once the reachable work is done and the
   mobile lines are landed or recorded as blocked with the toolchain each
   needs.

## Definition of done

**For the hardening work:**

- Every collision-table row produces its **expected** outcome on both targets:
  `E1013` rows answer a Rivet diagnostic (never a cargo failure), and the rows
  Layers 1-2 make legal build cleanly.
- The two targets **agree on every row** — a divergence is a bug even when
  both targets succeed for different reasons.
- The class test exists, runs both targets, and fails if a future change
  reintroduces a collision or a divergence.
- The `VERIFY FIRST` questions are answered empirically, and the commit body
  records what the probe showed.
- `rivet trace` still resolves the service function.
- Pillars 02 and 08 describe the handler module, the reserved prefix, and the
  scoped reserved-name rule.
- `./scripts/gate.sh` passes over the union, and the test count rose by the
  number of new tests.

**For every change:**

- `./scripts/gate.sh` passes (fmt, clippy `-D warnings`, tests, `cargo deny`,
  the example build and audit, repo self-checks).
- Pipeline changes are verified end to end: rebuild `examples/basic`, run the
  binary, `curl` the affected routes. WASM changes also run under `wasmtime`.
- Affected docs are updated (pillar doc, ROADMAP boxes, README).
- Tracker lines move from `todo.md` to `completed.md` with real commit hashes.
- `make clean` has run if the session generated large build output.
