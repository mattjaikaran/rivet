# Rivet resume prompt

Paste this into a fresh session to continue work on Rivet with full context.
Read it, then follow the steps below in order.

---

You are the senior engineer continuing work on **Rivet**, a public open-source
Rust repository at `~/dev/rivet` (branch `main`). Mission: turn a Python DSL
into a compiled, memory-safe Rust API server, and publish it.

**The generated-crate name-collision defect class is closed** at `d134335`.
The generator no longer accepts a DSL identifier that collides with a name
the generated crate owns, both targets agree on every input, and a class test
holds the invariant. Do not reopen it — but read "What the probe changed" so
you do not rebuild a wrong model of it.

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

Five commits, plus the tracker:

| Commit | What |
| :--- | :--- |
| `c605370` | `rivet-core/src/reserved.rs`: the reserved names and the constants the generator emits, in one place |
| `9edacd1` | Handlers moved into `mod handlers`; every generator-owned symbol prefixed `rivet_`; `#[allow(non_snake_case)]` on items carrying a user name |
| `5b3976b` | `E1013` in `parser/python.rs`, scoped per namespace |
| `1c297f9` | `commands/build/tests/collisions.rs`: the class test, both targets |
| `f4ceb97` | Pillars 02 and 08 and the README layout line |

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

## What the probe changed — do not rebuild the wrong model

The previous handoff was wrong in both directions, twice. The probe is the
authority; a table you are handed is not.

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
- **Four checks already landed**, and they are the pattern to follow:
  `E2013` (a feature flag set without its feature), `E2014` (two routes on one
  method and path), `E1012` (two routes sharing a handler name), and `E1013`
  (an identifier colliding with a generated name).
- `./scripts/gate.sh` is the acceptance bar: fmt, clippy `-D warnings`, **302
  tests**, `cargo deny`, the example build and audit, repo self-checks. Green
  at `d134335`.
- Tracker coherent: 6 todo items, 148 completed.

### Environment

- `wasm32-wasip1` and `wasmtime` (Homebrew) are installed; both are needed for
  the WASM acceptance.
- Mobile toolchains are **absent**: no JDK, no `kotlinc`, no Android SDK, no
  `uniffi-bindgen`; `swiftc` and Xcode exist.

## Next up

1. **The remaining three `rust_native_features` flags.** Each needs its layer:
   - `zero_copy_deserialization` — teach the parser to set
     `FieldDefinition::is_borrowed` and render a borrowed `&str` with a
     lifetime; then flip the flag. This is the only one that is reachable now.
   - `raii_connections` — needs a database surface in the DSL. Blocked.
   - `compile_time_rbac` — needs a way to mark a route protected; the
     decorator accepts only `path` and `stories`. The decorator must grow a
     keyword first.
   A flag set without its feature is already `E2013`.
2. **The mobile deliverables**, blocked on toolchains: UniFFI bindings for
   Kotlin and Swift, then `rivet mobile init --platforms ios,android`. If the
   toolchains appear, build them over `mod service` so the binding surface and
   the HTTP surface cannot drift. Record each as blocked with the toolchain it
   needs rather than ticking it.
3. **Phase-5 docs and tracker close**, once the reachable work is done and the
   mobile lines are landed or recorded as blocked.

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
2. `rtk ./scripts/gate.sh` — expect green, 302 tests, 6 todo, 148 completed.
3. Read `rivet-core/src/reserved.rs` in full, then
   `rivet-cli/src/transpiler/rust/handler.rs`. Those two files hold the
   invariant; the rest of the generator follows from them.
4. Pick the next roadmap item. `zero_copy_deserialization` is the reachable
   one: it needs `FieldDefinition::is_borrowed` set by the parser and a
   borrowed render with a lifetime, then the flag flips.
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
