# Rivet resume prompt

Paste this into a fresh session to continue work on Rivet with full context.
Read it, then follow the steps below in order.

---

You are the senior engineer continuing work on **Rivet**, a public open-source
Rust repository at `~/dev/rivet` (branch `main`). Mission: turn a Python DSL
into a compiled, memory-safe Rust API server, and publish it.

**This session has one priority: close a real correctness defect class before
any roadmap work.** A previous session shipped features whose defects were
found by review and probing, not by the test suite. The suite has no
adversarial-input tests, so the generator accepts DSL input that parses,
passes the Gauntlet, and then produces a Rust crate that cannot compile. Nine
such inputs are confirmed below. Close the class, add the guard that would
have caught it, and then continue the roadmap.

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

## The defect class you must close

### Root cause

The generated crate is **one flat Rust namespace**, and two things write into
it: the DSL, and the generator. A handler lands beside `fn main`; a DTO lands
beside `String`, `Vec`, and `Json`; a DTO name lands in the *type* namespace
that `mod service` and `mod channel` also occupy — which is why
`class service:` fails to compile. Nothing reserves the generator's names, so
a user who picks one gets cargo errors against generated code instead of a
Rivet diagnostic.

There is a second, worse symptom: **the two targets disagree**. The native
generator emits handlers at the top level; the WASM generator emits them
inside `mod service`. So the same blueprint gets two different answers.

### Confirmed cases (verified at `9d77297`)

Reproduce any of these by writing the snippet to `app.py` next to a
`rivet.toml` that holds only `[project] name = "p"`, then running
`rivet build [--target wasm] app.py`.

| Input | Native | WASM |
| :--- | :--- | :--- |
| a handler named `main` | cargo fail | builds |
| a handler named `json_obj` | cargo fail | builds |
| a handler named `json_number` | cargo fail | builds |
| a handler named `channel_error` | cargo fail | builds |
| a DTO class named `String` | cargo fail | cargo fail |
| a DTO class named `Vec` | cargo fail | cargo fail |
| a DTO class named `Json` | cargo fail | builds |
| a DTO class named `service` | cargo fail | cargo fail |
| a DTO class named `serde_json` | cargo fail | cargo fail |

`cargo fail` means `rivet build` printed `Build succeeded`-style progress,
ran cargo, and cargo reported errors — typically `E0428` (defined multiple
times) or a type that resolves to the user's DTO instead of the intended one.

### The fix is one invariant, not nine patches

> Every name the generated crate emits belongs to the generator. A DSL
> identifier that would collide with one is rejected, with a line number,
> before any crate is written — and both targets reject the same inputs.

The invariant holds in three layers: make the collisions structurally
impossible where you can, and validate the small remainder where you cannot.

**Layer 1 — namespace the handlers.** Emit native handlers inside
`mod handlers` (the WASM target already does exactly this) and register them
as `handlers::ping::<channel::InProcess>`. This kills every
handler-versus-helper collision at once and makes the two targets symmetric.
It does not replace `E1012`: two routes sharing a handler name are still
rejected, because a module cannot hold one name twice.

**Layer 2 — prefix everything the generator owns.** Rename the runtime
helpers and internal modules to a reserved prefix:
`rivet_json_obj`, `rivet_json_number`, `rivet_channel_error`,
`rivet_to_json`, `rivet_dispatch`, `rivet_body_text`, `rivet_answer`,
`rivet_json_error`, `rivet_allowed_methods`, `RIVET_DECLARED_PATHS`,
`mod rivet_executor`, `mod rivet_fixed_array`. Keep the *public* generated
surface readable — `mod service`, `mod channel`, `mod admin`, and the DTO
structs stay as they are, because the pillars document them.

**Layer 3 — one guard for the residual set.** After layers 1 and 2, the names
that can still collide are the crate-level ones: the documented modules and
the types the generated code names. Add **`E1013`** in
`rivet-cli/src/parser/python.rs`, so it carries a real file and line and every
command rejects the module (`build`, `audit`, `trace`, `plan`, `mcp`), not
only the two generators. Reject any **handler name or DTO name** in the
reserved set, and any that starts with `rivet_`/`RIVET_`.

Do **not** apply the reserved-set check to DTO *field* names. A field lives
inside its struct, and `pub main: String` inside `pub struct Foo` compiles —
rejecting it would refuse working input. Field names need only the keyword
check that `E1011` already performs (`type: str` is rejected today).

### Derive the reserved set, so it cannot rot

A hand-maintained denylist rots the moment someone adds a helper. Fix that by
making the list and the emission share one source of truth:
**`rivet-core/src/reserved.rs`** exports the reserved names *and* the
constants the generator emits, so a rename cannot drift from the check:

```rust
/// Names the generated crate emits and a DSL identifier may not reuse.
pub const HANDLERS_MODULE: &str = "handlers";
pub const PREFIX: &str = "rivet_";

/// Everything the generated code names: the documented modules, the
/// helpers' prefix, and the types the emitter writes by name.
pub const RESERVED: &[&str] = &[
    // Generated modules (pillar 02 documents service and channel).
    "service", "channel", "admin", "assets", "discovery", HANDLERS_MODULE,
    // Symbols the crate root already holds.
    "main",
    // Types the generated code writes by name (Option only when a field is
    // Optional, so it is reserved unconditionally rather than conditionally).
    "String", "Vec", "Option", "Json", "bool", "i64", "f64",
    // Paths the generated code writes by name.
    "serde", "serde_json", "axum", "tokio", "tracing", "std",
];
```

Then:

- the generators emit `reserved::HANDLERS_MODULE` rather than a literal, so
  the module name has one definition;
- the parser checks the set plus `PREFIX` for every user identifier;
- a unit test in `reserved.rs` asserts that `RESERVED` contains every module
  name the generator constants define, so adding a module without reserving it
  fails a test rather than shipping.

That is what makes this bounded instead of open-ended: the set is exactly the
names the emitter writes, and the emitter is what defines them.

### The class test — the guard that was missing

Add a table-driven integration test at
`rivet-cli/src/commands/build/tests/collisions.rs`, registered in
`commands/build/tests.rs`.

**Each row carries its expected outcome**, because the fix changes what is
legal. Layers 1 and 2 make the handler collisions and the helper-module
collisions *build*; Layer 3 rejects what remains. A test that asserts a
diagnostic for every row would fail against the fix.

| Input | Expected | Why |
| :--- | :--- | :--- |
| handler `main` | builds | `mod handlers` holds it, away from `fn main` |
| handler `json_obj`, `json_number`, `channel_error` | builds | helper names carry the prefix |
| DTO `fixed_array`, `executor` | builds | those modules are `rivet_*` now |
| handler `rivet_anything` | `E1013` | the prefix is reserved |
| DTO `rivet_anything` | `E1013` | the prefix is reserved |
| DTO `String`, `Vec`, `Option`, `Json` | `E1013` | the emitter writes those types by name |
| DTO `service`, `channel`, `admin`, `assets`, `discovery`, `handlers` | `E1013` | the generated modules own those names |
| DTO `serde_json`, `serde`, `axum`, `tokio`, `tracing`, `std` | `E1013` | the generated code writes those paths |

Assert per row, on **both targets**:

1. the actual outcome matches the expected one;
2. **native and WASM agree.** That is the invariant, and it is the assertion
   that matters most — the original defect included the two targets
   disagreeing, and a per-target test would have let that ship.

For every `E1013` row, also assert the diagnostic names the offending
identifier and carries a line. Do not add rows for DTO *field* names: a field
lives inside its struct and cannot collide at crate level.

This test is the actual fix. Those nine cases existed because every generator
test fed the generator names it had chosen itself; the suite never asked what
happens when the user chooses.

### Cases that are not defects — do not "fix" these

- A handler named `Router`: Rust keeps types and values in separate
  namespaces (`use axum::Router` is a type import).
- A DTO named `Type`: not a Rust keyword.
- A handler named `dispatch`, `to_json`, `answer`, `json_error`, `body_text`,
  or `allowed_methods`: these are WASM-only helpers, and the WASM target
  namespaces handlers, so they pass today. Layer 1 makes the native target
  behave the same way. They are not reserved names — reserving them would
  reject input that is fine.

`Option` is the one judgment call. A DTO named `Option` compiles today **only
because no field anywhere in the blueprint is `Optional[...]`** — the
generator writes `Option<{base}>` only for an optional field, so the name
collides the moment someone adds one. That makes the rule unpredictable: the
same DTO name is legal or illegal depending on an unrelated field. Reserve
`Option` unconditionally, and say why in the code comment. Predictable beats
permissive here, and `E1013` is a cheap correction for a user who hits it.

## What is verified working — do not break it

- **Phases 0-4 complete.** Phase 4 closed 2026-09-10: the compile-time plugin
  system, the multi-service transport switch, `rivet dev`, the embedded
  static assets, the admin panel, service discovery (Consul and etcd), and
  `rivet sync` (Jira and Linear).
- **Phase 5 started.** The WASM target: `rivet build --target wasm` emits a
  `wasm32-wasip1` command module that shares the native target's `mod service`
  and DTO structs, carries no axum/tokio/gRPC/plugins/assets, and answers the
  blueprint's routes. Verified under Wasmtime. The `const_generics` flag:
  `List[float, 768]` renders `[f64; 768]` with a generated serde bridge,
  because the derive stops at 32 elements.
- **Two blueprint checks already landed** and are the pattern to follow:
  `E2014` (two routes on one method and path — the native router panicked at
  startup and the WASM dispatch silently kept the first arm) and `E1012` (two
  routes sharing a handler name — cargo reported "defined multiple times").
- `./scripts/gate.sh` is the acceptance bar: fmt, clippy `-D warnings`, **287
  tests**, `cargo deny`, the example build and audit, and the repo
  self-checks. It is green at `9d77297`.
- Tracker coherent: 6 todo items, 135 completed.

### Environment

- `wasm32-wasip1` and `wasmtime` (Homebrew) are installed. Both are needed for
  the WASM acceptance; the module's real-platform check reports a missing host
  instead of passing silently.
- Mobile toolchains are **absent**: no JDK, no `kotlinc`, no Android SDK, no
  `uniffi-bindgen`; `swiftc` and Xcode exist.

## How to work: subagents

The hardening work is genuinely parallel. Decide the contract first, then fan
out.

**Contract — decide all of it before spawning anything, and put it in the
batch `context` so every subagent builds the same shape:**

- the reserved prefix (`rivet_` / `RIVET_`);
- the handler module name (`handlers`) and the constant that defines it in
  `rivet-core/src/reserved.rs`;
- the exact `RESERVED` list from the section above, and the rule that the
  generators emit `reserved::*` constants instead of literals;
- the diagnostic code (`E1013`) and that it is raised in the parser, not the
  generators, and applies to handler and DTO names only — never field names;
- that every row must behave identically on both targets;
- that **`{service}` stays emitted before `{handlers}`** in the assembled
  `main.rs`. `rivet trace` resolves a route's logic at runtime by scanning the
  generated source line by line (it looks for `fn {handler}(`), so moving the
  handler block ahead of the service block silently breaks it. If your design
  needs a different order, `trace.rs` must change in the same commit.

**Wave 1 — three file-disjoint slices, one `task` batch:**

1. `NativeNamespacing` — owns
   `rivet-cli/src/transpiler/rust/{main_file,handler,helpers,service}.rs`,
   `rivet-cli/src/commands/trace.rs`, and every call site that hardcodes the
   current registration: `rivet-cli/src/transpiler/rust/tests.rs` (the
   `.route("/ping", get(ping::<channel::InProcess>))` assertion),
   `rivet-cli/src/transpiler/rust/tests/features.rs` (the
   `post(create_ping::<channel::InProcess>)` assertion), and the trace-output
   assertion in `commands/trace.rs`. Emit the handlers inside `mod handlers`,
   rename the helpers to the reserved prefix, update the router registration,
   and update those assertions. `trace.rs`'s route *lookup* matches `.route(`
   followed by the path, so the lookup survives the rename; the assertion and
   the emitted block order are what need attention.
   Acceptance: `examples/basic` builds, `rivet trace` still finds the service
   function, and a handler named `json_obj` no longer reaches cargo.
2. `WasmNamespacing` — `rivet-cli/src/transpiler/rust/wasm.rs` and
   `rivet-cli/src/transpiler/rust/wasm/tests.rs`. Apply the same prefix to its
   helpers and internal modules, and take the module names from `reserved`.
   Acceptance: the WASM module still answers under `wasmtime run`.
3. `ReservedNames` — new `rivet-core/src/reserved.rs`, plus
   `rivet-cli/src/parser/python.rs` and `parser/python/tests.rs`. Define the
   constants and `RESERVED`, raise `E1013`, and add the unit test that keeps
   `RESERVED` in step with the generator's module constants. Acceptance: the
   parser rejects `rivet_x`, `String`, and `service` with `E1013` and a line.

These three touch disjoint files, so they may run at once. Slice 3 defines
`reserved.rs`, which slices 1 and 2 consume — give all three the exact
constant names in the contract so they can compile against the agreed shape
without waiting on each other.

**Wave 2 — after wave 1 lands:** `CollisionClassTest` — new
`rivet-cli/src/commands/build/tests/collisions.rs` plus the test-module
registration in `commands/build/tests.rs`. It asserts the combined output of
wave 1, so it cannot start until that output exists.

**Wave 3 — the integration commit:** docs (`docs/pillars/02-*`,
`docs/pillars/08-*`, `README.md` if layout changes), tracker, resume prompt.
Do this yourself, not by subagent.

Rules for every batch:

- **Skip validation in subagents.** Tell each one explicitly: do not run
  `./scripts/gate.sh`, `cargo clippy`, or the full test suite; do not run
  formatters. Run those once, yourself, over the union of the changes.
- Each subagent gets exact file paths, the contract above, and an acceptance
  criterion stated as an observable result — a specific test command and its
  expected output, or `cargo build` failing with a named error.
- Give the task in `# Target` / `# Change` / `# Acceptance` form.
- Never let a subagent make a design decision. Never let one edit a file
  another one owns; if two need the same file, serialize them.
- A read-only question about unfamiliar code goes to a `scout` subagent, not a
  chain of reads.
- If a spawn fails with `No model selected`, that is an environment fault, not
  a task fault: work the slices inline and keep the same file discipline.

## How to work: tokens and `rtk`

Route long output through `rtk` and never paste it back raw:

```bash
rtk ./scripts/gate.sh
rtk cargo test --workspace
rtk cargo clippy -- -D warnings
rtk git log --oneline -20
rtk git diff --stat
```

Use surgical reads (`read` with `offset`/`limit`) and never re-read what you
already hold. Prefer `target/debug/rivet` while iterating; the cargo cache is
warm, so the gate takes about 100 seconds.

## How to work: commits in a public repository

People will read this history. Every commit must stand on its own.

- **One commit per coherent unit.** Conventional Commits prefix, capitalized
  imperative subject, 50 characters or fewer, no trailing period, blank line,
  body wrapped at 72.
- **The body says what and why, not how.** The diff shows how.
- **Never commit a literal placeholder.** Do not stage `<pending>` or `TBD`
  while waiting for a hash. Land the feature commit first, then fill the real
  hash in a follow-up tracker commit.
- **Never commit build output.** `.gitignore` covers `target/`, `generated/`,
  and `generated-wasm/`; check `git status --short` before every commit. The
  one deliberate exception is `examples/basic/dist/`, which the asset test
  renames.
- **A fix commit names the symptom and the cause.** "Reject two routes on one
  method and path" with a body that quotes the panic is a good commit. "Fix
  bug" is not.
- **Do not commit a diagnostic whose code is already taken.** Check the code
  tables (`parser/python.rs` for `E1xxx`, `gauntlet/mod.rs` for the rule
  codes) and the whole tree (`grep -rho '"E[0-9]\{4\}"' rivet-cli/src | sort
  -u`) before you pick one. A previous session shipped `E1010` twice; it was
  caught in review, not by the suite.
- **Verify before you commit.** Run the specific test or smoke test that
  covers the change. Run `./scripts/gate.sh` once over the union of the
  session's changes, not per file. For the WASM target, run the module under
  `wasmtime` — a unit test on generated text is not proof.
- **Clean up before you stop:** `make clean` (or `make clean-all` when disk is
  tight). Never leave generated crates behind.

## House rules

- **Fix at the source.** No stubs, placeholders, `TODO` shims, speculative
  abstractions, duplicated logic, dead code, or invented facts.
- **A bug that surfaces as cargo's `E2009` is a generator bug**, not a user
  bug. Give it its own diagnostic. The newest examples are `E1012` in the
  parser (a duplicate handler name) and `E2011`-`E2014` in the generator
  (array shapes, the optional fixed array, an unimplemented flag, and
  overlapping routes).
- **Put a guard where it has the most context.** A check that needs a file and
  line goes in the parser, so every command rejects the input. A check about
  rendered output goes in the generator, and must be called by *both*
  generators.
- **One pattern per concern.** Match the existing module layout, error-code
  ranges (`E1xxx` parser, `E2xxx` generator and Gauntlet, `E3xxx` context
  engine and agentic commands), diagnostics, and naming.
- **Rust best practices.** Idiomatic ownership over clones; small fallible
  functions; `Result` with structured errors; no panics in library paths; no
  `unsafe` without a soundness comment; no `unwrap`/`expect` outside tests;
  build JSON by hand from `serde_json::Value` (`json!` is clippy-banned).
- **File ceiling is 400 lines** (`constraint-tools` enforces it). Split
  before you reach it, into sibling modules with a `tests.rs`.
- **This repo is public.** No secrets, placeholders, or personal content.
  Provider keys live in the environment only.

## Process failures, and what to change

### Why the suite missed this

The gate was green while nine inputs produced unbuildable crates. The reason
is narrow and worth naming, because it will repeat if you do not change it:

**Every generator test fed the generator a name the test author chose.** The
fixtures use `ping`, `echo`, `OrderCreate`, `Embedding`. No test asked what
happens when the *user* chooses the name, and the Gauntlet cannot see the
question either — it reads the DSL, and `def json_obj` is a perfectly ordinary
Python function. The failure only exists after generation, so it needs a test
that generates and compiles.

Three habits follow, and they apply to the rest of the roadmap:

1. **A test that only supplies inputs you picked proves the happy path.** For
   anything that renders user input into another language, add a row per
   *shape of input* that the user controls: names, empty values, unicode, the
   boundary lengths. The class test is the model.
2. **A green gate is necessary, not sufficient.** The gate proves the fixtures
   compile. It cannot prove that an input outside the fixtures does. When you
   add a generator capability, spend one command probing its edges — the
   probe scripts in this session's history took about two minutes each and
   found six cases the suite had missed.

3. **Reproduce before you fix, and do not trust a table you were handed.**
   The table above was produced by running the cases. Claims in this project
   have been wrong in both directions: one review said a DTO named `service`
   was untested when it had just been probed and does fail cargo, and other
   reviews reported as open several defects that were already fixed. Run the
   case yourself, then fix it.

### Other defects this project survived, so they are not repeated

- **`.gitignore` held a bare `build/`**, which also matched
  `rivet-cli/src/commands/build/`. Every integration test in that directory —
  transport, assets, the admin panel, discovery — was untracked, so a fresh
  clone ran the gate without them. Fixed in `4263fc3`. `git status --short`
  is not enough to catch this class: use `git check-ignore -v <path>`.
- **An output directory was nearly committed.**
  `examples/basic/generated-wasm/` arrived with the WASM target and was in
  neither `.gitignore` nor `scripts/clean.sh`. `git check-ignore -v` caught
  it, and both were fixed in the same commit (`ddd4d45`). When you add an
  output directory, update `.gitignore` and the clean script together.
- **A diagnostic code shipped twice** (`E1010`: response-type mismatch, then
  a duplicate-handler check). Fixed in `08c8b1a`. Check uniqueness against the
  code table and the whole tree before you commit a new code.
- **A tracker entry cited a hash whose code no longer matched**, because a fix
  landed after the entry was written. When a follow-up changes what an entry
  describes, cite both commits or the later one (`9d77297`).

## First steps in the session

1. `git status` and `rtk git log --oneline -10` to confirm the checkout.
2. `rtk ./scripts/gate.sh` to confirm the baseline is green (expect 287
   tests, 6 todo items, 135 completed).
3. **Reproduce two cases from the table yourself** — one native
   (`handler_json_obj`), one that diverges (`dto_Json` on both targets). Do
   not take the table on trust; the previous session's tables have been wrong
   before.
4. Read `rivet-cli/src/transpiler/rust/main_file.rs`,
   `rivet-cli/src/transpiler/rust/wasm.rs`, and
   `rivet-cli/src/parser/python.rs` far enough to place the three layers of
   the fix.
5. Write the contract (reserved names, prefix, module name, `E1013`), then run
   wave 1 as one `task` batch with three subagents.
6. Land wave 1, then wave 2 (the class test), then verify: the table must come
   back `DIAG:E1013` (or the specific code for the shape) on **both** targets,
   with no `CARGO-FAIL` row.
7. Update docs and the tracker in the integration commit. Move any finished
   line from `tasks/todo.md` to `tasks/completed.md` with its real commit
   hash.
8. Only then continue the roadmap, at "Next up".

## Next up, after the defect class is closed

1. **The remaining three `rust_native_features` flags.** Each needs its layer
   first:
   - `zero_copy_deserialization` — teach the parser to set
     `FieldDefinition::is_borrowed` and render a borrowed `&str` with a
     lifetime; then flip the flag.
   - `raii_connections` — needs a database surface in the DSL, which does not
     exist. Blocked until one is in scope.
   - `compile_time_rbac` — needs a way to mark a route protected; the
     decorator accepts only `path` and `stories` today.
   A flag set without its feature is already `E2013`, so the config cannot
   over-claim.
2. **The mobile deliverables**, blocked on toolchains: UniFFI bindings for
   Kotlin and Swift, then `rivet mobile init --platforms ios,android`. If the
   toolchains appear, build the bindings over `mod service` so the binding
   surface and the HTTP surface cannot drift.
3. **Phase-5 docs and tracker close**, once the reachable work is done and the
   mobile lines are landed or recorded as blocked with the toolchain each one
   needs.

## Definition of done

**For the hardening work:**

- Every row of the collision table produces its **expected** outcome on both
  targets: the `E1013` rows answer a Rivet diagnostic (never a cargo
  failure), and the rows Layers 1-2 make legal build cleanly.
- The two targets **agree on every row**. This is the invariant, and it is the
  assertion that matters most: a divergence is a bug even when both targets
  happen to succeed for different reasons.
- The class test exists, runs both targets, and fails if a future change
  reintroduces a collision or a divergence.
- `rivet trace` still resolves the service function, so the handler module did
  not break the generated-source scan.
- Pillars 02 and 08 describe the handler module, the reserved prefix, and the
  reserved-name rule.
- `./scripts/gate.sh` passes over the union of the changes, and the test count
  rose by the number of new tests.

**For every change:**

- `./scripts/gate.sh` passes (fmt, clippy `-D warnings`, tests, `cargo deny`,
  the example build and audit, repo self-checks).
- Pipeline changes are verified end to end: rebuild `examples/basic`, run the
  binary, and `curl` the affected routes. WASM changes also run the module
  under `wasmtime`.
- Affected docs are updated (pillar doc, ROADMAP boxes, README).
- Tracker lines move from `todo.md` to `completed.md` with real commit hashes.
- `make clean` has run if the session generated large build output.
