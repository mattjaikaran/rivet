# Rivet resume prompt

Paste this into a fresh session to continue work on Rivet with full context.
Read it, then follow the steps below in order.

---

You are the senior engineer continuing work on **Rivet**, a public open-source
Rust repository at `~/dev/rivet` (branch `main`). Mission: turn a Python DSL
into a compiled, memory-safe Rust API server, and publish it.

## Scope discipline — read this before you pick up anything

**The Python front end is not finished. Finish it before any other language.**

A previous session wrote "the reachable Python-side work is done" and moved on
to the `rust_native_features` flags and the mobile bindings. That claim was
**wrong**. It was true only of that session's own tracker section. The Python
front end is still a phase-0 subset, and the roadmap's real work is in it.

The correct order is:

1. **Python front-end features** — this is the work. See the gap table below.
2. TypeScript DSL front end — only after Python is complete, and only with
   explicit scope.
3. Kotlin, Swift, Java, Go, C#, and every other target — parked. Do not shape
   the IR for them.

Two side-quests are legitimately parked because they need a layer the DSL does
not have: `raii_connections` (no database surface) and `compile_time_rbac`
(no way to mark a route protected). The mobile bindings are parked on absent
toolchains. None of those is a reason to leave the Python front end.

If you catch yourself reading `docs/ROADMAP.md` phase 5 deliverables or
thinking about another language, stop and come back to the gap table.

## Start state

At `0a1c11a`, pushed to `origin/main`, working tree clean and **green**.

```bash
rtk git log --oneline -5
rtk ./scripts/gate.sh          # expect green: fmt, clippy, 284 rivet-cli tests
```

Two things to confirm before you start, because both were true at handoff:

- `git status --short` prints nothing.
- `cargo build --workspace` succeeds.

**A previous session ended with a tree that did not compile.** Reverting was
deliberate, not a mistake: an unbuildable tree is a worse handoff than a green
one plus a written design. The design it reverted is recorded below, so
nothing is lost.

## What is actually missing from the Python front end

`README.md` "What Rivet transpiles" lists the subset honestly. Here is what
that subset excludes, verified in the code, not assumed:

| Capability | State | Evidence |
| :--- | :--- | :--- |
| Path parameters `/orders/{id}` | **rejected** | `parser/decorator.rs` `validate_path`: "uses path parameters, which are not supported yet" |
| Query parameters | **absent** | no `Query` handling anywhere in `rivet-cli/src` |
| More than one parameter | **rejected** | `parser/signature.rs`: "phase 0 supports a single request-body parameter" |
| Assignment in a handler | **rejected** | `parser/body.rs`: "`{kind}` statements are not supported in handler bodies yet" |
| Control flow (`if`, `for`, `match`) | **rejected** | same |
| More than one `return` | **rejected** | `parser/body.rs`: "multiple return statements are not supported yet" |
| Calls, attribute access, arithmetic, f-strings, comprehensions | **rejected** | `parser/expr.rs` |

A handler today is **one `return`** of literals, a parameter, or a single DTO
constructor. That is the entire language. Every row above is Python-side
front-end work and is in scope.

**Order them yourself, but path parameters are first**, because a REST
framework that cannot serve `GET /orders/{id}` is not usable, and because the
IR decision it forces (where a route parameter lives, and how the parser binds
it to a handler parameter) is the shape every later feature extends.

## This session's target: path parameters

Make `@api.get("/orders/{id}")` with `def get_order(id: int)` build and answer
on **both** targets.

### The probe — these results are the specification

A probe in a scratch crate settled how axum 0.8 actually behaves. It was
outside the repo and is **not committed**, so it is recorded here. Re-run it if
you doubt a row; these are measured, not recalled.

Four extractor forms compile: `Path<i64>`, `Path<(i64, String)>` for two
params, `Path<String>`, and `Path<i64>` combined with a body extractor.

Extractor order is **`Path`, then `State`, then the consuming extractor**.
That composes for a JSON body, for a borrowed `Bytes` body, and for no body:

```rust
async fn json_body(
    Path(id): Path<i64>,
    State(state): State<u32>,
    Json(note): Json<Note>,
) -> Json<serde_json::Value>
```

Measured status codes, which the WASI target must reproduce:

| Request | Result |
| :--- | :--- |
| `GET /one/42` | `200 {"id":42}` |
| `GET /two/7/x` | `200 {"a":7,"b":"x"}` |
| `POST /mix/5` with a JSON body | `200` — Path and body together |
| `GET /one/abc` where the type is `i64` | **`400`** `Invalid URL: Cannot parse \`abc\` to a \`i64\`` |
| `GET /one` (missing segment) | **`404`** |
| `GET /one/1/2` (extra segment) | **`404`** |
| `GET /one/42/` (trailing slash) | **`404`** |
| `GET /one//42` (empty segment) | **`404`** |
| `GET /s/` (empty parameter) | **`404`** |
| `POST /one/42` | `405` |
| `GET /ONE/42` | `404` — case-sensitive |

Three of those drive the design:

1. **A bad segment type is `400`, not `404`.** The route matched; the value
   failed to parse. A generator that answers `404` here breaks parity.
2. **Segment count is exact.** A trailing slash and an empty segment both
   miss. The WASI matcher must split on `/` and compare counts, and must not
   treat an empty segment as a match.
3. **Percent-decoding happens per segment, before typing.** Measured:
   `%20` → space, `%2F` → `/` (so decode *after* splitting, or an encoded
   slash becomes a separator), `%C3%A9` → `é`, and `+` is **not** decoded
   (it stays `+`, unlike form encoding). For `i64`, `007` → `7`, `+1` → `1`,
   `-1` → `-1`.

### The IR change (design, reverted so the tree stayed green)

Add to `rivet-core/src/ir.rs`, beside `RequestSpec`:

```rust
/// One `{name}` placeholder in a route path, with the type its handler
/// declares for it.
///
/// The order matches the placeholders in [`RouteDefinition::path`], because
/// the generated router binds them positionally. The parser resolves the type
/// from the handler signature and rejects a placeholder the handler does not
/// declare, so every entry here has a matching parameter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathParam {
    /// The placeholder name, without the braces.
    pub name: String,
    /// The type the handler declares for it.
    pub ty: TypeRef,
}
```

and one field on `RouteDefinition`:

```rust
    /// `{name}` placeholders in `path`, in path order, with the type the
    /// handler declares for each. Empty for a path with no placeholders.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path_params: Vec<PathParam>,
```

`#[serde(default)]` keeps every serialized blueprint and fixture compatible.
The parser resolves the type; the type is *not* re-parsed in the generator.

### The trap that costs an hour if you miss it

Adding a field to `RouteDefinition` breaks **every struct literal that
constructs one**. There are **16** across the repo. Find them with the `grep`
tool — pattern `RouteDefinition \{`, directory `.`, `gitignore: true`. The
harness blocks shell `grep`, so do not try `xargs grep`.

The compiler reports them one at a time and stops at the first, so you cannot
read the whole list from one build. Count them up front or you will loop.
`cargo build --workspace 2>&1 | rtk err` shows only the error line.

Most sites are tests and fixtures and want `path_params: vec![]`. Only
`rivet-cli/src/parser/python.rs` wants a real value.

**Do not reach for `ast_edit` here.** It was tried: the pattern
`RouteDefinition { $$$REST }` matched 5 of the 16 sites and the proposal could
not be inspected field by field. Plain edits, verified by the compiler, are
correct for this shape.

## Work plan

Decompose this into genuinely independent slices. Do not serialize work that
does not depend on itself.

**Wave 1 — run these together.**

- **Own the design yourself.** Write the IR change, then fix the 16 sites so
  the tree compiles. Nothing else can start until it does.
- **Spawn one `scout` (read-only)** on the mapping question, because the
  answer is spread across the generator and you should not read it all
  yourself. Ask it for, per file: the function name, its line range, its
  current signature, and exactly what a leading path-parameter argument does
  to it. Targets: `transpiler/rust/service.rs` (`render_route`, `render_fn`),
  `channel.rs` (`Method`, `render_trait`, `render_in_process`,
  `render_grpc_impl`, `render_dispatch`), `handler.rs`,
  `wasm.rs` (`render_arm`, `render_paths`, `RIVET_DECLARED_PATHS`),
  `rust.rs` (`render_router`), `admin.rs` (the route table), and whatever
  `rivet trace` scans to resolve a route.

**Wave 2 — after the IR compiles, in parallel, one writer per file.**

Declare the contract up front, in the batch `context`, before you spawn.
Suggested contract: a path parameter becomes a **leading** parameter of the
service function and the same argument on the channel method, typed by
`PathParam::ty`, in path order.

| Slice | Files it owns | Agent |
| :--- | :--- | :--- |
| Parser: accept `{name}`, bind it to a typed parameter, reject a missing or duplicate placeholder | `parser/decorator.rs`, `parser/signature.rs`, `parser/python.rs` | you |
| Native: `Path` extractor in order, the router, the service and channel argument | `transpiler/rust/handler.rs`, `service.rs`, `channel.rs`, `rust.rs` | `task` |
| WASI: match a parameterised path, percent-decode, reproduce the status codes above | `transpiler/rust/wasm.rs` | `task` |
| Tests: parser, generator, and the integration rows | the `tests/` trees | `task` |

**File discipline is the rule that makes this work.** No two writers touch one
file. If two slices need `service.rs`, one of them owns it and the other
coordinates over `hub`. A mid-flight build failure is expected while siblings
edit; tell every agent to skip validation and you run the suite once at the
end.

Every task in the batch must be told to skip `cargo fmt`, `clippy`, and the
full test suite. Run those once, over the union, yourself.

**Wave 3 — verify end to end, on both targets.** A green gate is necessary,
not sufficient.

```bash
# native
./target/release/rivet build examples/basic/app.py
./examples/basic/generated/target/release/basic &
curl -i localhost:3000/orders/42      # 200
curl -i localhost:3000/orders/abc     # 400, not 404
curl -i localhost:3000/orders         # 404

# WASI
./target/release/rivet build --target wasm examples/basic/app.py
echo '{"method":"GET","path":"/orders/42"}' | wasmtime run \
  examples/basic/generated-wasm/target/wasm32-wasip1/release/basic.wasm
```

## How to work: `rtk`

`rtk` is a token-filtering proxy in front of ordinary commands. It routes most
commands through and compresses the output, and it has dedicated filters. Use
it for everything that can produce a wall of text.

```bash
rtk ./scripts/gate.sh              # passes through; the gate is the bar
rtk cargo test --workspace         # test output, summarized
rtk test cargo test -p rivet-cli   # only failures, plus a SUMMARY block
rtk err cargo build --workspace    # only errors and warnings
rtk git log --oneline -20
rtk git diff                       # condensed, changed lines only
rtk diff                           # ultra-condensed working diff
rtk read path/to/file.rs
rtk find . -name '*.rs'
rtk tree src
rtk json                           # compact JSON, or --keys-only
```

`rtk test` and `rtk err` are the two that matter most here: a failing
`cargo test` is thousands of lines, and you only need the failure.

Prefer `target/release/rivet` while iterating. The cache is warm, so the gate
takes about 100-400 seconds depending on how much recompiles.

## How to work: subagents

Agent types available: `scout` (read-only, fast, for exploration), `task`
(general, can edit), `reviewer`, `security-reviewer`, `sonic` (strictly
mechanical edits and data collection only). Spawn several in one `task` call
with a `tasks[]` array; they run concurrently.

Rules that keep this cheap and correct:

- **`scout` for every "where is this and what does it look like" question.**
  It is read-only and returns compressed context. Do not read six files
  yourself when one scout summarizes them.
- **`sonic` for strictly mechanical work** — adding a field to 16 literals,
  renaming a symbol across fixtures. No judgment, no design.
- **`task` for a slice that must investigate and edit in one pass.**
- **Give every subagent the contract, not a hint.** It has no conversation
  history. A one-line prompt produces a plausible wrong answer.
- **Name one owner per file.** Siblings that must share a file coordinate
  through `hub` before editing, not after.
- **Do not parallelize what is sequential.** The IR must compile before the
  renderers can be written against it.
- Keep the batch inside the concurrency cap and give each task a stable
  CamelCase name.

## House rules

- **Fix at the source.** No stubs, placeholders, `TODO` shims, or speculative
  abstractions.
- **A bug that surfaces as a cargo error is a generator bug.** Give it its own
  diagnostic. Path-parameter errors belong in the `E1xxx` parser range. The
  codes in use at `0a1c11a` are `E1001`-`E1014`, `E2002`-`E2016`, `E2999`,
  and `E3xxx`, so **the next free parser code is `E1015`**. `E1014` is taken
  by the borrowed-body work; do not reach for it out of habit. Grep the tree
  for a code before you claim it.
- **Put a guard where it has the most context.** A check needing a file and a
  line goes in the parser, so every command rejects the input. A check about
  rendered output goes in the generator and must be called by **both**
  generators.
- **The generated crate must compile warning-free** for every input the change
  makes legal. An item carrying a user-chosen name takes
  `#[allow(non_snake_case)]`.
- **File ceiling is 400 lines** and it bites. Three files went over it in one
  session. Split into a sibling module with a `tests.rs` before you reach it.
- **No two conventions.** Read the neighbouring module before you add one.
- Idiomatic ownership over clones; `Result` with structured errors; no panics
  in library paths; no `unwrap`/`expect` outside tests; build JSON by hand
  from `serde_json::Value` (`json!` is clippy-banned).
- **This repo is public.** No secrets, placeholders, or personal content.

## Commits

People read this history. One coherent unit per commit; Conventional Commits
prefix; capitalized imperative subject, 50 characters or fewer, no trailing
period; blank line; body wrapped at 72. The body says what and why, not how.

A fix commit names the symptom and the cause and **quotes the actual error**.
When a fix rests on a probe, record the probe in the body and say so when the
probe contradicts the plan — that has already happened twice in this repo, and
the correction is the valuable part.

Never commit a placeholder. Land the feature, then fill the real hash in a
follow-up tracker commit. Never commit build output; `git check-ignore -v
<path>` is the test, not `git status`.

## Definition of done

- `rtk ./scripts/gate.sh` passes: fmt, clippy `-D warnings`, `cargo test
  --workspace`, `cargo deny`, the example build and audit, repo self-checks.
- Pipeline changes are verified end to end: rebuild `examples/basic`, run the
  binary, `curl` the affected routes. WASM changes also run under `wasmtime`.
- The generated crate compiles with no warnings for every input the change
  makes legal.
- Affected docs are updated: `README.md` "What Rivet transpiles", the relevant
  pillar, `docs/ROADMAP.md`.
- The tracker line moves from `tasks/todo.md` to `tasks/completed.md` with the
  date and the real commit hash.
- `make clean` has run if the session generated large output.

## First steps

1. `rtk git log --oneline -5`, `git status --short`, and
   `rtk ./scripts/gate.sh` — confirm green at `0a1c11a`.
2. Read `rivet-core/src/ir.rs` (`RouteDefinition`, `PathParam` goes beside
   `RequestSpec`), then `rivet-cli/src/parser/decorator.rs` `validate_path`
   and `rivet-cli/src/parser/signature.rs` `parse_parameters`. Those three
   hold the parse-side decision.
3. Write the IR change and fix the 16 `RouteDefinition` sites until the tree
   compiles. This is step one of the feature, not setup.
4. Spawn the `scout` for the generator mapping while you do step 3.
5. Then decompose Wave 2, declare the contract, and spawn it.
