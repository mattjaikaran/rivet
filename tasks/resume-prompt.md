# Rivet resume prompt

Paste this into a fresh session to continue work on Rivet with full context.
Read it, then follow the steps below in order.

---

You are the senior engineer continuing work on **Rivet**, a public open-source
Rust repository at `~/dev/rivet` (branch `main`). Mission: turn a Python DSL
into a compiled, memory-safe Rust API server, and publish it.

## Scope discipline — read this before you pick up anything

**The Python front end is not finished. Finish it before any other language.**

Three earlier sessions shipped a real feature and then wrote that the
"reachable Python-side work is done". Each claim was true only of that
session's own tracker section. The front end is still a documented subset
with hard rejection sites, and `tasks/todo.md` says so in its own words.

The correct order is:

1. **Python front-end features** — this is the work. See the gap table below,
   and the seed prompt `prompts/prompt-09-python-frontend.md`, which covers
   both remaining items in detail.
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

`main` carries the front-end commits above `6315ebb` — the one the `for`/`match`
feature landed at — and `origin/main` carries the same tip. Working tree clean.

```bash
rtk git log --oneline -5
git status --short                 # expect: prints nothing
rtk ./scripts/gate.sh              # expect green: fmt, clippy, tests, deny, example, self-checks
```

The gate is green at that tip. It takes about 130 to 400 seconds when the test
and release profiles are warm. A cold profile takes far longer, because
`cargo test --workspace` pulls `lance` and `datafusion` into the test profile.
Run it first; do not inherit a claim you did not verify.

## What is actually missing from the Python front end

The two feature items in `tasks/todo.md` under "Python front-end completion"
(that section also carries `while` as its own open item, covered below), with
the evidence verified in the code, not assumed:

| Item | State | Evidence (file:line) |
| :--- | :--- | :--- |
| Calls, attribute access, f-strings, comprehensions | **rejected** | `parser/expr.rs:186` attribute access; `expr.rs:132` "only DTO constructors may be called"; `expr.rs:190` the generic subset arm, which catches comprehensions and subscripts; `expr/strings.rs:17` f-strings |
| `//` and `%` | **rejected** | `parser/expr/operator.rs:84` — `` "the operator `{operator}` means something different in Rust than in Python for negative values" `` (`E1007`) |

`for` and `match` shipped at `6315ebb`. A handler body now holds assignments,
`if`/`elif`/`else`, `for`, `match`, and `return`, over literals, request
values, a DTO constructor, `+ - * /`, comparisons, `and`, `or`, and `not`.

**`while` stays out, and the decision is recorded here on purpose.**
`tasks/todo.md` carries it as its own open item. `while` is rejected by the
`other` arm at `parser/body.rs:114` with `E1006`, and the Verifier complexity
rule already counts `while_statement` (`verifier/complexity.rs:37`), so it was
always expected to land. It needs no new IR beyond a condition and a body, and
it needs the same `all_paths_return` treatment `for` got: a `while` may run
zero times, so it never completes a handler. Add it when the work plan below
puts it in scope, or record why it stays out. Do not discover it halfway
through.

**A probe already settled how `//` and `%` must render, and it rules out the
obvious answer**, so do not repeat it from memory when you reach item 3.
Rust's `div_euclid`/`rem_euclid` agree with Python only when the divisor is
**positive**: for `7 // -2` Python answers `-4` and `div_euclid` answers
`-3`. The `tasks/todo.md` acceptance for that item tests only positive
divisors, so a naive `div_euclid` rendering passes it while still being
wrong. The measured table is in `prompts/prompt-09-python-frontend.md`.

## This session's target: calls, attribute access, f-strings, comprehensions

Make each of these build and answer on **both** targets, or refuse it with a
diagnostic that names the construct. Order them yourself by risk, but the
cheap one is first:

**f-strings are the likely first win.** `f"{a}-{b}"` lowers to Rust
`format!`, and the interpolated pieces are already in the expression subset.
`parser/expr/strings.rs:17` rejects the `f` prefix today. Accept an f-string
whose interpolated expressions are expressions the subset already carries, and
refuse one that carries anything else.

**Attribute access and general calls need a design decision, and it is the
task.** `Expr` has no field-access node, and a call has no callee beyond a DTO
name:

- what does a callable mean in the IR? A method on a DTO, a helper, a runtime
  class? Each answer changes whether the generated Rust needs a trait, a free
  function, or a macro.
- an attribute read needs a borrow-versus-copy decision, and the DTO borrow
  rules already exist: `borrowed[str]` renders `&'a str`, so a field read must
  not force a copy the DTO avoided.

**A comprehension over a list is a loop in disguise.** `Stmt::For` exists now,
so lower a comprehension to it rather than inventing a second iteration
construct. Reach that after the loop shape is understood.

### What the code already gives you

These are verified. Do not re-derive them; do check them if you edit nearby.

- **The WASM target reuses the native service renderer.** `transpiler/rust/wasm.rs:64`
  calls `service::render_service_module`, and `wasm/template.rs:9` says so in
  prose. A new statement rendered once in `transpiler/rust/body.rs` therefore
  covers **both** targets. Do not write a second renderer.
- **The Verifier already counts `for` and `match`.** `verifier/complexity.rs:32`
  maps `for_statement`, `while_statement`, and `case_clause`. The rule walks
  the **tree-sitter syntax tree**, not the IR, so it sees a new statement the
  moment the parser accepts it. No complexity change is needed.
- **`Stmt` is a five-variant enum**: `Return(Expr)`, `Assign{name, ty, value}`,
  `If{branches, otherwise}`, `For{name, ty, iterable, body}`, and
  `Match{subject, arms}` (`rivet-core/src/ir/expr.rs:146`).
- **`Expr` has eleven variants**: `Null`, `Bool`, `Int`, `Float`, `Str`,
  `Array`, `Object`, `Ident`, `Construct`, `Binary`, `Not`
  (`rivet-core/src/ir/expr.rs:113`). A new `Expr` variant is a **wider** break
  than a new `Stmt` variant, because more sites match on it, and four of those
  sites carry a catch-all arm. See trap 1.

### The traps that cost an hour each

**1. Three sites match on `Expr` exhaustively, and four more have a catch-all
arm.** The three stop the build, one at a time, so you cannot read the whole
list from one build. The four catch-all sites compile in silence and then
mishandle the new variant, which is the worse failure. Both lists are verified
as of `6315ebb`:

| Site | Exhaustive? | What it does |
| :--- | :--- | :--- |
| `rivet-core/src/infer.rs:61` | yes | resolves the type of an expression |
| `transpiler/rust/expression.rs:215` | yes | renders an expression as `serde_json::Value` |
| `transpiler/rust.rs:606` (`expr_kind`) | yes | names the kind of an expression, for a diagnostic |
| `transpiler/rust/expression.rs:21` (`render_named`) | no — `_ =>` | renders a named (DTO) response |
| `transpiler/rust/expression.rs:106` (`render_typed`) | no — `_ => {}` | renders an expression into a target type |
| `transpiler/rust.rs:478` (`visit`) | no — `_ => {}` | counts identifier uses, for the clone decision |
| `parser/validate/dto.rs:119` | no — `_ =>` | checks an expression against a DTO field type |

Give each of the four a real arm for the new variant. A catch-all arm is where
a new expression kind goes wrong without a compiler error.

**2. A new `Expr` variant must reach the identifier counter, or a value moves
twice.** `count_idents_in` (`transpiler/rust.rs:470`) decides whether a
non-`Copy` parameter is cloned, and its inner `visit` (`rust.rs:477`) is one of
the catch-all sites above: an attribute read that hides an identifier from that
walk means the value is not cloned, moves twice, and the generated crate fails
with `E0382`. The counter weights a use inside a loop body twice and exempts
the innermost loop's own binding — read the doc comment at `rust.rs:461` before
you extend it.

**3. Keep the fall-through tail intact.** `render_route`
(`transpiler/rust/service.rs:133`) appends `serde_json::Value::Null` to a
`-> dict` body that can fall off its end, because Python answers `None` there.
A new statement that ends a body must not defeat `Stmt::all_paths_return`, or
the generated function ends in `()` against a `serde_json::Value` return type
and fails with `E0308`. `all_paths_return` lives at
`rivet-core/src/ir/expr.rs:200`.

**4. A guard belongs where it has the most context.** A check that needs a
file and a line belongs in the parser, so every command rejects the input. A
check about rendered output belongs in the generator, and both generators must
call it.

**5. Do not reach for `ast_edit` on the enum variants.** A previous session
tried it on a structurally similar change and the pattern matched a fraction
of the sites, with a proposal that could not be inspected field by field.
Plain edits, verified by the compiler, are correct for enum variants.

**6. Watch the file ceiling while you edit.** `parser/body.rs` is 354 lines,
`parser/body/flow.rs` is 357, and `transpiler/rust/body.rs` is 238, so all three
have room, but a fourth statement kind adds to `body/flow.rs` too. Split into a
sibling module with a `tests.rs` before you reach 400.

## Work plan

Decompose into genuinely independent slices. Do not serialize work that does
not depend on itself.

**Wave 1 — run these together.**

- **Own the design decision yourself.** What a callable means in the IR shapes
  everything else. Write the `Expr` variant, update `infer`, then fix the
  seven exhaustive-match sites until the tree compiles. Nothing else can start
  until it does.
- **Spawn one `scout` (read-only)** on the rendering question, because the
  answer is spread across the generator. Ask it, per file: the function name,
  its line range, and exactly what rendering an attribute read requires.
  Targets: `transpiler/rust/expression.rs` (`render_typed`, `render_value`,
  `render_named`), `transpiler/rust/borrow.rs` (the borrowed-field lifetime),
  and `parser/validate/dto.rs` (the field-type check).

**Wave 2 — after the IR compiles, in parallel, one writer per file.**

Declare the contract up front, in the batch `context`, before you spawn.

| Slice | Files it owns | Agent |
| :--- | :--- | :--- |
| f-strings: accept the `f` prefix, lower to `format!`, refuse a piece the subset cannot render | `parser/expr/strings.rs`, `parser/expr.rs` | you |
| Attribute access and calls | `transpiler/rust/expression.rs`, `parser/validate/dto.rs` | `task` |
| Tests: parser unit tests plus a native and a WASI integration row | the `tests/` trees, `examples/basic/app.py` | `task` |

**File discipline is the rule that makes this work.** No two writers touch one
file. If two slices need `expr.rs`, one owns it and the other coordinates over
`hub`. A mid-flight build failure is expected while siblings edit; tell every
agent to skip validation and you run the suite once at the end.

Every task in the batch must be told to skip `cargo fmt`, `clippy`, and the
full test suite. Run those once, over the union, yourself.

**Wave 3 — verify end to end, on both targets.** A green gate is necessary,
not sufficient. Assert the **body**, not the status code.

```bash
# native
./target/release/rivet build examples/basic/app.py
./examples/basic/generated/target/release/basic &
curl -i localhost:3000/<the new route>       # 200, and the expected body

# WASI
./target/release/rivet build --target wasm examples/basic/app.py
echo '{"method":"GET","path":"<the new route>"}' | wasmtime run \
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

Prefer `target/release/rivet` while iterating. Note that a `--bin` target has
no library, so `cargo test -p rivet-cli --lib` fails with "no library targets
found"; use `cargo test -p rivet-cli --bin rivet` instead.

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
  diagnostic, or fix the parser so the input is refused. Parser errors belong
  in the `E1xxx` range. The codes in use at `6315ebb` are `E1001`-`E1017`,
  `E2002`-`E2006`, `E2009`-`E2018`, `E2042`-`E2046`, `E2999`, and
  `E3000`-`E3023`, so **the next free parser code is `E1018`**, the next free
  generator code is `E2019`, and the next free context code is `E3024`. Grep
  the tree for a code before you claim it: an earlier resume prompt named
  `E1015` as free and the path-parameter work had already taken it, and this
  one named `E1016` while the `for`/`match` work took it.
- **Put a guard where it has the most context.** A check needing a file and a
  line goes in the parser, so every command rejects the input. A check about
  rendered output goes in the generator and must be called by **both**
  generators.
- **The generated crate must compile warning-free** for every input the change
  makes legal. An unused handler parameter warns today, and that is the one
  known exception: the user can act on it, unlike a generated name.
- **File ceiling is 400 lines** and it bites. Three files are grandfathered
  ratchets, and the numbers in
  `constraint-tools/src/bin/check-file-length.rs` are the authority, not their
  current size: `rivet-cli/src/parser/python.rs` (728),
  `rivet-cli/src/transpiler/rust.rs` (699), and
  `rivet-cli/src/commands/audit.rs` (679). A ratchet may not grow, but it has
  real headroom below its limit, so read the table before you decide a needed
  edit requires a split. Any other file, including a new module, takes 400.
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
probe contradicts the plan — that has already happened three times in this
repo, and the correction is the valuable part.

Never commit a placeholder. Land the feature, then fill the real hash in a
follow-up tracker commit. Never commit build output; `git check-ignore -v
<path>` is the test, not `git status`.

## Definition of done

- `rtk ./scripts/gate.sh` passes: fmt, clippy `-D warnings`, `cargo test
  --workspace`, `cargo deny`, the example build and audit, repo self-checks.
- Pipeline changes are verified end to end: rebuild `examples/basic`, run the
  binary, `curl` the affected routes and assert the body. WASM changes also
  run under `wasmtime`.
- The generated crate compiles with no warnings for every input the change
  makes legal.
- Affected docs are updated: `README.md` "What Rivet transpiles", the relevant
  pillar, `docs/ROADMAP.md`.
- The tracker line moves from `tasks/todo.md` to `tasks/completed.md` with the
  date and the real commit hash.
- `make clean` has run if the session generated large output.

## First steps

1. `rtk git log --oneline -5`, `git status --short`, and
   `rtk ./scripts/gate.sh` — confirm green at `6315ebb`.
2. Read `rivet-cli/src/parser/expr.rs` (the dispatch at `:34`, the call arm at
   `:120`, attribute access at `:184`), then
   `rivet-cli/src/parser/expr/strings.rs` (the f-prefix refusal at `:17`), then
   `rivet-cli/src/transpiler/rust/expression.rs:103` (`render_typed`). Those
   three hold the whole change.
3. Decide what a callable means in the IR, then write the `Expr` variant and
   fix the three exhaustive-match sites and the four catch-all sites from trap
   1 until the tree compiles. This is step one of the feature, not setup.
4. Spawn the `scout` for the rendering mapping while you do step 3.
5. Then decompose Wave 2, declare the contract, and spawn it.
