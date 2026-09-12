# Rivet resume prompt

Paste this into a fresh session to continue work on Rivet with full context.
Read it, then follow the steps below in order.

---

You are the senior engineer continuing work on **Rivet**, a public open-source
Rust repository at `~/dev/rivet` (branch `main`). Mission: turn a Python DSL
into a compiled, memory-safe Rust API server, and publish it.

## Scope discipline — read this before you pick up anything

**The Python front end is not finished. Finish it before any other language.**

Two earlier sessions shipped a real feature and then wrote that the
"reachable Python-side work is done". Both claims were true only of that
session's own tracker section. The front end is still a documented subset
with hard rejection sites, and `tasks/todo.md` says so in its own words.

The correct order is:

1. **Python front-end features** — this is the work. See the gap table below,
   and the seed prompt `prompts/prompt-09-python-frontend.md`, which covers
   all three items in detail.
2. TypeScript DSL front end — only after Python is complete, and only with
   explicit scope.
3. Kotlin, Swift, Java, Go, C#, and every other target — parked. Do not shape
   the IR for them.

Three side-quests are legitimately parked because each needs a layer the DSL
does not have: `raii_connections` (no database surface), `compile_time_rbac`
(no way to mark a route protected), and the mobile bindings (absent
toolchains). None of those is a reason to leave the Python front end.

If you catch yourself reading `docs/ROADMAP.md` phase-5 deliverables or
thinking about another language, stop and come back to the gap table.

## Start state

`main` at `a12e582`, pushed to `origin/main`, working tree clean.

```bash
rtk git log --oneline -5
git status --short                 # expect: prints nothing
rtk ./scripts/gate.sh              # expect green: fmt, clippy, tests, deny, example, self-checks
```

The last full green gate ran at `c9c549e`. The three commits after it
(`4686469`, `c9c549e`, `a12e582`) touch documentation and `scripts/clean.sh`
only — no `.rs` file — so the gate is expected to pass unchanged. Run it
first anyway; do not inherit a claim you did not verify.

## What is actually missing from the Python front end

The three open items in `tasks/todo.md` under "Python front-end completion",
with the evidence verified in the code, not assumed:

| Item | State | Evidence (file:line) |
| :--- | :--- | :--- |
| `for` and `match` in a handler body | **rejected** | both hit the `other` arm at `parser/body.rs:99` — `` "`{other}` statements are not supported in handler bodies yet" `` (`E1006`), which prints the node kind (`for_statement`, `match_statement`) |
| Calls, attribute access, f-strings, comprehensions | **rejected** | `parser/expr.rs:184` attribute access; `expr.rs:129` "only DTO constructors may be called"; `expr/strings.rs:17` f-strings; `expr.rs:188` the generic subset arm |
| `//` and `%` | **rejected** | `parser/expr/operator.rs:79` — `` "the operator `{operator}` means something different in Rust than in Python for negative values" `` (`E1007`) |

`while` is rejected by that same `other` arm, and `tasks/todo.md` does not
list it. The Verifier complexity rule already counts `while_statement`
(`verifier/complexity.rs:36`), so it was always expected to land. Scope it
explicitly with `for`/`match`, or record why it stays out — do not discover
it halfway through.

Everything else in phases 0-4 has shipped. A handler today holds assignments,
`if`/`elif`/`else`, and `return`, over literals, request values, a DTO
constructor, `+ - * /`, comparisons, `and`, `or`, and `not`. That is the
whole statement and expression language.

**A probe already settled how `//` and `%` must render, and it rules out the
obvious answer**, so do not repeat it from memory when you reach item 3.
Rust's `div_euclid`/`rem_euclid` agree with Python only when the divisor is
**positive**: for `7 // -2` Python answers `-4` and `div_euclid` answers
`-3`. The `tasks/todo.md` acceptance for that item tests only positive
divisors, so a naive `div_euclid` rendering passes it while still being
wrong. The measured table is in `prompts/prompt-09-python-frontend.md`.

**Order them yourself, but `for` and `match` are first**, because a REST
framework that cannot iterate or branch on a value cannot express real
business logic, and because the IR decision they force (how a nested block is
represented, and how a loop's body is walked) is the shape the remaining two
items extend.

## This session's target: `for` and `match`

Make these build and answer on **both** targets:

```python
@api.get("/orders/{id}/total", stories=["US-007"])
def order_total(id: int, quantity: int) -> dict:
    total = 0
    for line in [1, 2, 3]:
        total = total + line
    return {"id": id, "total": total}


@api.get("/grade/{score}", stories=["US-008"])
def grade(score: int) -> dict:
    match score:
        case 1:
            return {"grade": "low"}
        case _:
            return {"grade": "high"}
```

### What the code already gives you

These are verified. Do not re-derive them; do check them if you edit nearby.

- **The WASM target reuses the native service renderer.** `transpiler/rust/wasm.rs:64`
  calls `service::render_service_module`, and `wasm/template.rs:9` says so in
  prose. A new statement rendered once in `transpiler/rust/service.rs`
  therefore covers **both** targets. Do not write a second renderer.
- **The Verifier already counts `for` and `match`.** `verifier/complexity.rs:36`
  maps `"for_statement"`, and its module doc lists `case_clause` of a
  `match`. The rule walks the **tree-sitter syntax tree**, not the IR, so it
  sees the new statements the moment the parser accepts them. No complexity
  change is needed — but confirm it rather than assume it.
- **`Stmt` is a three-variant enum**: `Return(Expr)`, `Assign{name, ty, value}`,
  `If{branches, otherwise}` (`rivet-core/src/ir/expr.rs:146`). `Expr` has
  `Null`, `Bool`, `Int`, `Float`, `Str`, `Array`, `Object`, `Ident`,
  `Construct`, `Binary`, `Not`.

### The IR change

Add variants to `Stmt` in `rivet-core/src/ir/expr.rs`, beside `If`. A loop
needs a binding name, the collection, and a body; a `match` needs the subject
and the arms:

```rust
    /// `for <name> in <iterable>:` — one body per iteration.
    For {
        name: String,
        /// The element type the loop binding takes, resolved by the parser so
        /// the generator needs no second type analysis.
        ty: TypeRef,
        iterable: Expr,
        body: Vec<Stmt>,
    },
    /// `match <subject>:` — one arm per `case`.
    Match {
        subject: Expr,
        /// Each arm is an optional pattern and the body it guards. `None` is
        /// the `case _` wildcard, which must come last.
        arms: Vec<(Option<Expr>, Vec<Stmt>)>,
    },
```

`TypeRef` is already a field of `Assign`, so its import in that module is
present. Keep the same public surface otherwise; this is an addition.

### The traps that cost an hour each

**1. Adding a `Stmt` variant breaks every exhaustive match on it.** The
compiler reports them one at a time and stops at the first, so you cannot read
the whole list from one build. These are the sites as of `a12e582`:

| Site | What it does |
| :--- | :--- |
| `transpiler/rust/service.rs:195` | renders `Assign` |
| `transpiler/rust/service.rs:208` | renders `Return` |
| `transpiler/rust/service.rs:225` | renders `If` |
| `transpiler/rust.rs:479` | counts literals for the borrow / const-generics checks |
| `parser/validate.rs:70` | walks the body to type-check every `return` |

Count them up front or you will loop. `cargo build --workspace 2>&1 | rtk err`
shows only the error line.

**2. `Stmt::walk` must recurse into the new bodies.**
`rivet-core/src/ir/expr.rs:193` returns every statement including nested
blocks, and two callers depend on it: the complexity rule and the literal
counter at `transpiler/rust.rs:479`. A `for` body that `walk` does not visit
means a string literal inside the loop is **not counted**, which silently
breaks the borrow and fixed-array checks. This is the single most likely
mistake in this change.

**3. `Stmt::all_paths_return` must handle both variants.**
`rivet-core/src/ir/expr.rs:175` decides whether a handler that returns a value
returns on every path. A `for` cannot complete — the collection may be empty —
so it never satisfies it. A `match` completes only when it has a `case _`
wildcard and every arm completes. Getting this wrong turns a non-returning
handler into a build error, or worse, accepts one that can fall off the end.

**4. Do not reach for `ast_edit` on the `Stmt` variants.** A previous session
tried it on a structurally similar change and the pattern matched a fraction
of the sites, with a proposal that could not be inspected field by field.
Plain edits, verified by the compiler, are correct for enum variants.

### The stale comment to fix while you are there

`parser/expr.rs:9` says the supported arithmetic is `` `+ - * / // %` ``, but
`parser/expr/operator.rs:79` **refuses** `//` and `%`, and `README.md` agrees
with the refusal. The module doc is wrong. Correct it in this change.

## Work plan

Decompose into genuinely independent slices. Do not serialize work that does
not depend on itself.

**Wave 1 — run these together.**

- **Own the design yourself.** Write the `Stmt` variants, update `walk` and
  `all_paths_return`, then fix the exhaustive-match sites so the tree
  compiles. Nothing else can start until it does.
- **Spawn one `scout` (read-only)** on the rendering question, because the
  answer is spread across the generator. Ask it, per file: the function name,
  its line range, and exactly what rendering a nested block requires.
  Targets: `transpiler/rust/service.rs` (`render_body`, `render_stmt`),
  `transpiler/rust.rs` (the literal counter), `parser/validate.rs` (the return
  walk), and `verifier/complexity.rs` (confirm `for`/`match` counting).

**Wave 2 — after the IR compiles, in parallel, one writer per file.**

Declare the contract up front, in the batch `context`, before you spawn.
Suggested contract: `for` renders as a Rust `for` loop over the rendered
iterable, binding with the parser-resolved type; `match` renders as a Rust
`match` whose `case _` becomes `_` and whose other arms compare by equality.
Both targets share one renderer, so `service.rs` is the only render site.

| Slice | Files it owns | Agent |
| :--- | :--- | :--- |
| Parser: accept `for` and `match`, resolve the loop binding's type, reject a non-final `case _` | `parser/body.rs` | you |
| Generator: render both statements, update the literal counter and the return walk | `transpiler/rust/service.rs`, `transpiler/rust.rs`, `parser/validate.rs` | `task` |
| Tests: parser unit tests plus a native and a WASI integration row | the `tests/` trees, `examples/basic/app.py` | `task` |

**File discipline is the rule that makes this work.** No two writers touch one
file. If two slices need `body.rs`, one owns it and the other coordinates over
`hub`. A mid-flight build failure is expected while siblings edit; tell every
agent to skip validation and you run the suite once at the end.

Every task in the batch must be told to skip `cargo fmt`, `clippy`, and the
full test suite. Run those once, over the union, yourself.

**Wave 3 — verify end to end, on both targets.** A green gate is necessary,
not sufficient.

```bash
# native
./target/release/rivet build examples/basic/app.py
./examples/basic/generated/target/release/basic &
curl -i localhost:3000/orders/1/total      # 200
curl -i localhost:3000/grade/1             # 200, the low arm

# WASI
./target/release/rivet build --target wasm examples/basic/app.py
echo '{"method":"GET","path":"/grade/1"}' | wasmtime run \
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
- **`sonic` for strictly mechanical work** — renaming a symbol across
  fixtures, adding a field to many literals. No judgment, no design.
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
  diagnostic. Parser errors belong in the `E1xxx` range. The codes in use at
  `a12e582` are `E1001`-`E1015`, `E2002`-`E2006`, `E2009`-`E2018`,
  `E2042`-`E2046`, `E2999`, and `E3000`-`E3023`, so **the next free parser
  code is `E1016`**, the next free generator code is `E2019`, and the next
  free context code is `E3024`. Grep the tree for a code before you claim it:
  the previous resume prompt named `E1015` as free and it was already taken by
  the path-parameter work.
- **Put a guard where it has the most context.** A check needing a file and a
  line goes in the parser, so every command rejects the input. A check about
  rendered output goes in the generator and must be called by **both**
  generators.
- **The generated crate must compile warning-free** for every input the change
  makes legal.
- **File ceiling is 400 lines** and it bites. The largest sources are
  `transpiler/rust.rs` (561, grandfathered, must shrink not grow),
  `parser/python.rs` (520, grandfathered), and `commands/audit.rs` (427).
  Split into a sibling module with a `tests.rs` before you reach it.
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
   `rtk ./scripts/gate.sh` — confirm green at `a12e582`.
2. Read `rivet-core/src/ir/expr.rs` (`Stmt` at :146, `walk` at :193,
   `all_paths_return` at :175), then `rivet-cli/src/parser/body.rs:63`
   (`parse_statement`, where `for`/`match` are rejected) and
   `rivet-cli/src/transpiler/rust/service.rs:185` (`render_body`). Those three
   hold the whole change.
3. Write the `Stmt` variants, update `walk` and `all_paths_return`, then fix
   the five exhaustive-match sites until the tree compiles. This is step one
   of the feature, not setup.
4. Spawn the `scout` for the rendering mapping while you do step 3.
5. Then decompose Wave 2, declare the contract, and spawn it.
