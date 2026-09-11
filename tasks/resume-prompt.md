# Rivet resume prompt

Paste this into a fresh session to continue work on Rivet with full context.
Read it, then follow the steps below in order.

---

You are the senior engineer continuing work on **Rivet**, a public open-source
Rust repository at `~/dev/rivet` (branch `main`). Mission: turn a Python DSL
into a compiled, memory-safe Rust API server, and publish it.

**Two defect classes are closed, and you must not reopen either.**

The generated-crate name-collision class closed at `d134335`: the generator
no longer accepts a DSL identifier that collides with a name the generated
crate owns, both targets agree on every input, and a class test holds the
invariant.

The `zero_copy_deserialization` flag landed at `df7eb2e`. A `borrowed[str]`
DTO field renders as `&'a str` behind `#[serde(borrow)]`, the parser sets the
IR marker, and three diagnostics (`E1014`, `E2015`, `E2016`) hold the rule
that a borrow is legal only as a route's request body. Read the section on it
below before you touch the borrowed path. Do not reopen it either.

**This session's priority: the remaining Python-side roadmap work.** The
Python front end is the only front end; finish every Python-side item before
any TypeScript work.

## Scope

Rivet ingests a Python/TypeScript DSL and emits Rust (`docs/ARCHITECTURE.md`).
**Only the Python front end exists today.**

In scope: the Python front end, phase 5's Python side (the WASM target —
landed — plus the native Kotlin/Swift bindings and the `rust_native_features`
flags), and the hardening work in this prompt.

Deferred: a **TypeScript DSL front end**, and the **UniFFI TypeScript (React
Native) binding**. Do not start either, and do not shape the IR for them,
without explicit scope.

The phase-4 admin panel is **not** TypeScript work: it ships as one static
file with no React or Solid build step. Keep it that way.

## What landed in the last session

Six commits, plus the tracker:

| Commit | What |
| :--- | :--- |
| `753e9f8` | `parser/annotation.rs`: `ParsedType` and the `borrowed[str]` parse |
| `27d3f04` | The borrowed render, the `Bytes` handler, and the borrow rules |
| `b45b3c5` | `#[allow(dead_code)]` on the unused monolith transport |
| `65a24ea` | The example route and the end-to-end proof on both targets |
| `daaab00` | Pillars 02 and 08, the README, and the roadmap paragraph |
| `df7eb2e` | The tracker entry with the probe's results |

### The invariant

> Every name the generated crate emits belongs to the generator. A DSL
> identifier that would collide with one is rejected, with a line number,
> before any crate is written — and both targets agree on every input.

Three layers hold it:

1. **Handlers live in `mod handlers`.** The router registers
   `handlers::ping::<channel::InProcess>`. A handler is a local function, and
   a local item shadows the crate root's `use super::*` glob without error,
   so `def service()` builds.
2. **Generator-owned symbols carry `rivet_`** — `rivet_json_obj`,
   `rivet_channel_error`, `rivet_dispatch`, `rivet_answer`,
   `rivet_body_text`, `rivet_json_error`, `rivet_allowed_methods`,
   `RIVET_DECLARED_PATHS`, `mod rivet_executor`, `mod rivet_fixed_array`.
   The public surface stays readable: `mod service`, `mod channel`,
   `mod admin`, `mod assets`, `mod discovery`, the DTO structs, `fn main`.
3. **`E1013` rejects the residual set**, raised in the parser so `build`,
   `audit`, `trace`, `plan`, and `mcp` all reject the module.

Two references still reach the crate root and each is written `super::…`,
because a local item does outrank an explicit path:
`super::State(channel): super::State<C>` and `super::Json(...)`. **Do not
"simplify" those to bare `State` or `Json`** — a handler named after either
extractor captures its own pattern (`E0532`) and the crate stops compiling.

### The scoping rule, and why it is scoped

The rule is **not** one list applied everywhere:

| Identifier | Rejected when |
| :--- | :--- |
| DTO name | `reserved::is_reserved_dto` — the full `RESERVED` set plus the prefix |
| handler name | `reserved::is_reserved_handler` — the prefix alone |
| DTO field name | never; `E1011` already covers a Rust keyword |

A handler named `service`, `String`, `State`, `Router`, or `main` is legal
and must stay legal. A DTO with any of those names is rejected. Read
`rivet-core/src/reserved.rs` before changing the rule; the scoping comments
there explain the namespace argument.

### The borrowed-body invariant

> A `borrowed[str]` field is a slice of the request body, so it has exactly
> one buffer to point into. It is legal only as a route's request body, and
> only when the project sets `zero_copy_deserialization`.

Four layers hold it:

1. **The parser marks the field.** `parse_type_text` returns a `ParsedType`
   carrying `is_borrowed`, and `parse_dto_class` writes it to
   `FieldDefinition`. The annotation parser lives in `parser/annotation.rs`;
   `parser/types.rs` holds only the DTO class walk now.
2. **`E1014` rejects an annotation that cannot borrow** — a bare
   `borrowed[str]` parameter, a borrowed return type, `Optional[borrowed[
   str]]`, `borrowed[int]`, and `List[borrowed[str]]`. It is raised in the
   parser, so every command rejects the module.
3. **`E2015` rejects the borrow without the flag**, and **`E2016` rejects it
   outside a request body** (a response type, or a field of another DTO).
   Both live in `transpiler/rust/borrow.rs` and both generators call them.
4. **The native handler takes `Bytes`, not `Json`.** `Json<T>` requires
   `T: DeserializeOwned`; a borrowed DTO is the opposite, and a probe proved
   `Json<Note<'_>>` does not compile at all. The handler calls
   `serde_json::from_slice` itself. **Do not "simplify" that back to the
   `Json` extractor.** The WASM module decodes from the body text it already
   holds.

The lifetime is one elided `'_` at every use site, because every legal
position is a parameter or a local. A JSON string that carries an escape
cannot borrow, and the route answers `400`; that is the promise holding, not
a defect.

## What the probes changed — do not rebuild the wrong model

A table you are handed is not the authority. Two probes have corrected a
handoff in this project, and both corrections are recorded below.

### The collision probe

The previous handoff was wrong in both directions, twice.

| Claim in the old document | What the probe showed |
| :--- | :--- |
| handler `json_obj` fails native, builds WASM | it fails **both** targets |
| handler `main`, `json_number`, `channel_error` fail native, build WASM | correct |
| DTO `Json` fails native, builds WASM | correct; DTO `State` and `Router` behave the same way |
| handler `State`/`Router`/`String` — "very likely does not collide" | handler **`State` failed native with `E0532`**: the `State(channel)` extractor pattern resolved to the user's own function. `Router` and `String` built. This is why the pattern is qualified `super::State` |

Verified outcomes at the parent commit, for the record: handler `main`,
`json_obj`, `json_number`, `channel_error`, `State` failed; handler
`service`, `String`, `Router` built; DTO `String`, `Vec`, `service`,
`serde_json` failed both targets; DTO `Json`, `State`, `Router` failed native
and built WASM.

After the fix: both targets answer `E1013` for every name in `RESERVED` and
for a `rivet_`-prefixed handler or DTO; both build all 24 legal handler names;
the native binary for a handler named `State` answers `200 {}` and `404`; the
WASM module for a handler named `json_obj` answers
`{"body":{},"status":200}` under `wasmtime run`.

### The zero-copy probe

The plan for `zero_copy_deserialization` was "render `Json<Dto<'a>>` and be
done". That does not compile: axum's `Json<T>` extractor requires
`T: DeserializeOwned`, and a borrowed DTO is the opposite. The probe's exact
message was `implementation of Deserialize is not general enough`. The fix is
`Bytes` plus `serde_json::from_slice`, and the same probe proved the field
points inside the extracted buffer.

Two more rows the plan did not predict. `Optional[borrowed[str]]` and
`List[borrowed[str]]` cannot be rendered, because the borrow and the wrapper
would have to share one lifetime, so the parser rejects them. And a DTO
carrying both a borrowed field and `List[float, 4]` answers `E2003` until the
config also sets `const_generics`: the two flags are independent, not
subsumed, and a test pins the composition.

The parallel probe of all 16 input rows is the model to copy. One row per
shape the user controls, every row in the background at once, one verdict
line each.

## What is verified working — do not break it

- **Phases 0-4 complete.** Phase 4 closed 2026-09-10: compile-time plugins, the
  multi-service transport switch, `rivet dev`, embedded static assets, the
  admin panel, service discovery (Consul and etcd), and `rivet sync` (Jira and
  Linear).
- **Phase 5 started.** `rivet build --target wasm` emits a `wasm32-wasip1`
  command module sharing the native target's `mod service` and DTO structs,
  carrying no axum/tokio/gRPC/plugins/assets, verified under Wasmtime. Two of
  the four `[rust_native_features]` flags have landed: `const_generics`
  renders `List[float, 768]` as `[f64; 768]` with a generated serde bridge
  (the derive stops at 32 elements), and `zero_copy_deserialization` renders
  `borrowed[str]` as `&'a str`.
- **Seven checks already landed**, and they are the pattern to follow:
  `E2013` (a feature flag set without its feature), `E2014` (two routes on one
  method and path), `E1012` (two routes sharing a handler name), `E1013`
  (an identifier colliding with a generated name), `E1014` (a borrow
  annotation that cannot work), `E2015` (a borrow without the flag), and
  `E2016` (a borrow outside a request body).
- `./scripts/gate.sh` is the acceptance bar: fmt, clippy `-D warnings`, **284
  tests** in `rivet-cli` plus the workspace suite, `cargo deny`, the example
  build and audit on both targets, repo self-checks. Green at `df7eb2e`.
- Tracker coherent: 5 todo items, 163 completed.
- **The file ceiling is 400 lines and it bites.** Three new-code additions
  pushed files over it in one session, so put new code in a sibling module
  from the start rather than discovering it in the gate.

### Environment

- `wasm32-wasip1` and `wasmtime` (Homebrew) are installed; both are needed for
  the WASM acceptance.
- Mobile toolchains are **absent**: no JDK, no `kotlinc`, no Android SDK, no
  `uniffi-bindgen`; `swiftc` and Xcode exist.

## Next up

1. **The two remaining `rust_native_features` flags, both blocked on a layer
   the DSL does not have.** Do not flip either flag until its layer exists; a
   flag set without its feature is already `E2013`.
   - `raii_connections` — needs a database surface in the DSL. There is no
     connection to pool today, so this one has no reachable entry point.
   - `compile_time_rbac` — needs a way to mark a route protected. The route
     decorator accepts only `path` and `stories`, so the decorator must grow
     a keyword first, and then the generated crate needs a typestate layer
     with no runtime role lookup.
2. **The mobile deliverables**, blocked on toolchains: UniFFI bindings for
   Kotlin and Swift, then `rivet mobile init --platforms ios,android`. If the
   toolchains appear, build them over `mod service` so the binding surface and
   the HTTP surface cannot drift. Record each as blocked with the toolchain it
   needs rather than ticking it.
   Because both flag layers and the mobile bindings need something that does
   not exist in this repository yet, **the reachable Python-side work is
   done.** The honest next step is the phase-5 close below, not a search for
   filler: if you cannot name the missing layer a task needs, say so and
   record it rather than building a stub.
3. **Phase-5 docs and tracker close**, once the mobile lines are landed or
   recorded as blocked. `docs/ROADMAP.md` and the tracker both carry the
   phase-5 boxes; check them against the code before you tick anything.

## How to work: tokens and `rtk`

```bash
rtk ./scripts/gate.sh
rtk cargo test --workspace
rtk cargo clippy -- -D warnings
rtk git log --oneline -20
rtk git diff --stat
```

Use surgical reads (`read` with `offset`/`limit`); never re-read what you
hold. Prefer `target/release/rivet` while iterating. The cache is warm, so the
gate takes about 100 seconds.

**Probe with a parallel shell loop.** A one-case probe costs about two minutes
because cargo compiles a fresh crate; running ten of them at once costs the
same wall time as one. Write the case table as a shell `case` block that emits
a DSL snippet, run every row in the background, and collect one verdict line
per row. That is how the collision matrix was settled.

## How to work: commits in a public repository

People will read this history. Each commit must stand on its own.

- **One commit per coherent unit.** Conventional Commits prefix, capitalized
  imperative subject, 50 chars or fewer, no trailing period, blank line, body
  wrapped at 72.
- **The body says what and why, not how.** The diff shows how.
- **A fix commit names the symptom and the cause.** Quote the actual error.
- **Record probe evidence in the body** when a fix rests on one, and say so
  when the probe contradicts the plan.
- **Never commit a placeholder.** Land the feature commit, then fill the real
  hash in a follow-up tracker commit.
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
- **A bug that surfaces as a cargo error is a generator bug.** Give it its own
  diagnostic. The newest are `E1013` (parser, reserved name) and `E2011`-`E2014`
  (generator: array shapes, the optional fixed array, an unimplemented flag,
  overlapping routes).
- **Put a guard where it has the most context.** A check needing a file and a
  line goes in the parser, so every command rejects the input. A check about
  rendered output goes in the generator and must be called by *both*
  generators.
- **The generated crate must compile warning-free.** A cargo lint against
  generated code is noise the user cannot act on, so an item that carries a
  user-chosen name takes `#[allow(non_snake_case)]`, and a template parameter
  that a valid blueprint may leave unused takes `#[allow(unused_variables)]`.
- **One pattern per concern.** Match the module layout, error-code ranges
  (`E1xxx` parser, `E2xxx` generator and Gauntlet, `E3xxx` context engine and
  agentic commands), and the diagnostics.
- **Rust best practices.** Idiomatic ownership over clones; small fallible
  functions; `Result` with structured errors; no panics in library paths; no
  `unsafe` without a soundness comment; no `unwrap`/`expect` outside tests;
  build JSON by hand from `serde_json::Value` (`json!` is clippy-banned).
- **File ceiling is 400 lines** (`constraint-tools` enforces it;
  `rivet-cli/src/parser/python.rs` is grandfathered at 728). Split before you
  reach it, into sibling modules with a `tests.rs`.
- **This repo is public.** No secrets, placeholders, or personal content.

## Process failures, and what to change

### Why the suite missed the collision class

**Every generator test fed the generator a name the test author chose.** The
fixtures use `ping`, `echo`, `OrderCreate`, `Embedding`. No test asked what
happens when the *user* chooses, and the Gauntlet cannot see the question —
`def json_obj` is an ordinary Python function. The failure only exists after
generation, so it needs a test that generates and compiles.

Three habits follow, for the rest of the roadmap:

1. **A test that only supplies inputs you picked proves the happy path.** For
   anything rendering user input into another language, add a row per *shape
   of input the user controls*: names, empty values, unicode, boundary
   lengths. The class test is the model — it derives its rows from
   `reserved::RESERVED`, so the table cannot go stale.
2. **A green gate is necessary, not sufficient.** It proves the fixtures
   compile; it cannot prove an input outside them does. Spend one parallel
   command probing a new capability's edges before you trust it.
3. **Reproduce before you fix, and do not trust a table you were handed.**
   Run the case, then fix it, and record what the probe showed.

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
  its own fix made half those rows legal. Read a class-test table and the
  layer descriptions together before starting; if they conflict, the table is
  right.

## First steps in the session

1. `git status` and `rtk git log --oneline -10` to confirm the checkout.
2. `rtk ./scripts/gate.sh` — expect green, 284 tests in `rivet-cli`, 5 todo,
   163 completed.
3. Read `rivet-core/src/reserved.rs` in full, then
   `rivet-cli/src/transpiler/rust/handler.rs`. Those two files hold the
   invariant; the rest of the generator follows from them.
4. Pick the next roadmap item. The reachable Python-side work is done, so
   read "Next up" and confirm that against the code before you start
   anything: the two remaining flags and the mobile bindings each need a
   layer this repository does not have yet.
5. Probe the new capability's edges before you trust it, and record the probe
   in the commit body.

## Definition of done

**For every change:**

- `./scripts/gate.sh` passes (fmt, clippy `-D warnings`, tests, `cargo deny`,
  the example build and audit, repo self-checks).
- Pipeline changes are verified end to end: rebuild `examples/basic`, run the
  binary, `curl` the affected routes. WASM changes also run under `wasmtime`.
- The generated crate compiles with no warnings for every input the change
  makes legal.
- Affected docs are updated (pillar doc, ROADMAP boxes, README).
- Tracker lines move from `todo.md` to `completed.md` with real commit hashes.
- `make clean` has run if the session generated large build output.
