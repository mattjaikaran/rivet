# Prompt 09: Complete the Python Front End

**Objective**: Finish the Python DSL statement and expression language, so a
handler can express real business logic instead of a single branch over
literals. Three constructs remain rejected; accept each one on both targets.

**Context**: The Python front end is a documented subset, not a finished
parser. `README.md` "What Rivet transpiles" lists what works, and every
excluded construct fails the build with a structured diagnostic rather than
mistranslating. That refusal is deliberate — a silent mistranslation of
business logic is worse than an error — and it is why these three gaps are
visible rather than latent.

This prompt closes them. It is the work `tasks/todo.md` tracks under
"Python front-end completion", and it comes before any other language front
end or any parked side-quest.

State at `a12e582`: a handler body holds `name = <expr>`, `if`/`elif`/`else`,
and `return`, over literals, request values, a DTO constructor, `+ - * /`,
comparisons, `and`, `or`, and `not`. That is the entire statement and
expression language.

---

## Tasks

### 1. `for` and `match` in a handler body

Rejected today at `rivet-cli/src/parser/body.rs:99` with `E1006`:
`` `{other}` statements are not supported in handler bodies yet ``.

Add two variants to `Stmt` in `rivet-core/src/ir/expr.rs` beside `If`: a loop
carries a binding name, the element type the parser resolved, the iterable,
and a body; a `match` carries a subject and arms, where an arm is an optional
pattern ( `None` is the `case _` wildcard) and a body.

`while` is rejected by that same `other` arm, and `tasks/todo.md` does not
list it. The Verifier complexity rule already counts `while_statement`
(`verifier/complexity.rs:36`), so it was always expected to land. Decide
explicitly whether it is in scope here, and record the decision; do not
discover it halfway through.

Three consequences fall out of the enum change and each one is silent if you
miss it:

- **`Stmt::walk`** (`rivet-core/src/ir/expr.rs:193`) must descend into the new
  bodies. Two callers depend on it — the Verifier complexity rule and the
  literal counter at `rivet-cli/src/transpiler/rust.rs:479`. A loop body that
  `walk` does not visit means a string literal inside it is not counted, which
  breaks the borrow and fixed-array checks without any error.
- **`Stmt::all_paths_return`** (`:175`) must handle both variants. A `for`
  cannot complete, because the collection may be empty. A `match` completes
  only when it has a `case _` and every arm completes.
- **Every exhaustive match on `Stmt`** needs a new arm. At `a12e582` those are
  `transpiler/rust/service.rs:195`, `:208`, `:225`, `transpiler/rust.rs:479`,
  and `parser/validate.rs:70`. The compiler reports them one at a time and
  stops at the first, so count them before you start or you will loop.

Rendering: the WASI target reuses the native renderer.
`transpiler/rust/wasm.rs:64` calls `service::render_service_module`, so a
statement rendered once in `transpiler/rust/service.rs` reaches **both**
targets. Do not write a second renderer.

The Verifier needs no change: `verifier/complexity.rs:36` already maps
`for_statement` and its module doc lists the `case_clause` of a `match`,
because the rule walks the tree-sitter tree rather than the IR.

### 2. Calls, attribute access, f-strings, and comprehensions

Rejected today at `rivet-cli/src/parser/expr.rs:184` (attribute access, `x.y`),
`:129` (anything but a DTO constructor as a callee), `:188` (the generic
subset arm, which catches comprehensions and subscripts), and
`rivet-cli/src/parser/expr/strings.rs:17` (f-strings).

Accept what the generated Rust can render faithfully and refuse what it
cannot, one construct at a time, each with its own diagnostic. An f-string is
the likely first win: `f"{a}-{b}"` lowers to Rust `format!`, and the
interpolated pieces are already in the expression subset. Attribute access and
general calls need a decision about what a callable means in the IR — a
method on a DTO, a helper, a runtime class — and that decision is this task's
design work, not an afterthought.

Keep the refusal for anything that would not round-trip. A comprehension over
a list is a loop in disguise; if task 1 landed, lowering it to `Stmt::For` is
preferable to inventing a second iteration construct.

### 3. `//` and `%`, with Python's floor semantics

Rejected today at `rivet-cli/src/parser/expr/operator.rs:79` with `E1007`:
`` the operator `{operator}` means something different in Rust than in Python
for negative values ``. `BinOp` (`rivet-core/src/ir/expr.rs:11`) carries no
variant for either, and its doc comment records why.

**A probe settles how to render them, and it rules out the obvious answer.**
Rust's `div_euclid`/`rem_euclid` are not a drop-in for Python's floor
division, because Euclidean division forces the remainder non-negative while
Python gives it the divisor's sign. Measured:

| `a, b` | Python `a // b` | Rust `a.div_euclid(b)` | Python `a % b` | Rust `a.rem_euclid(b)` |
| :--- | ---: | ---: | ---: | ---: |
| `7, 2` | 3 | 3 | 1 | 1 |
| `-7, 2` | -4 | -4 | 1 | 1 |
| `7, -2` | **-4** | **-3** | **-1** | **1** |
| `-7, -2` | **3** | **4** | **-1** | **1** |
| `7, 3` | 2 | 2 | 1 | 1 |
| `-7, 3` | -3 | -3 | 2 | 2 |

They agree when the divisor is positive and disagree when it is negative.
Re-run the probe before you rely on the table; it is six lines of `rustc`.

So there are two honest options: emit a helper that computes the floor result
directly (floor division is `(a - b * floor(a/b))`-shaped and the checked
arithmetic must not overflow), or accept the operator only when the divisor
proves positive at the point of use and keep refusing otherwise. Either is
defensible; picking one and documenting why is the task.

**The acceptance criterion in `tasks/todo.md` is too weak to catch the bug.**
It requires `-7 // 2` to answer `-4` and `-7 % 3` to answer `2`. Both have a
positive divisor, so a naive `div_euclid` implementation passes while still
answering `-3` for `7 // -2`. Add a negative-divisor case to the tests
whatever you choose.

### 4. Docs and tracker

- Correct `rivet-cli/src/parser/expr.rs:9`, whose module doc lists `` `//` ``
  and `` `%` `` as supported arithmetic while `operator.rs:79` refuses them,
  and `README.md` agrees with the refusal.
- Update `README.md` "What Rivet transpiles" for each construct you accept,
  and remove the corresponding rejection bullet.
- Update `docs/ROADMAP.md` if a deliverable moves.
- Move each finished line from `tasks/todo.md` to `tasks/completed.md` in the
  commit that finishes it, with the date and the real hash.

---

## Acceptance Criteria

- A handler that loops over a list, and one that matches a value, build and
  answer on the native target and under WASI.
- A loop body containing a string literal is counted by the borrow and
  fixed-array checks; a `walk` that misses it is a bug, not a nicety.
- A `for` never satisfies `all_paths_return`; a `match` satisfies it only with
  a `case _` and completing arms.
- Rejected constructs keep a diagnostic that names the construct and carries a
  non-empty `suggested_fix`.
- If `//` and `%` are accepted, the tests include a negative divisor, and the
  chosen rendering is documented with the reason it differs from
  `div_euclid`.
- The generated crate compiles warning-free for every input the change makes
  legal, on both targets.
- `cargo fmt --all -- --check`, `cargo clippy -- -D warnings`, and
  `cargo test --workspace` all pass, and `./scripts/gate.sh` is green.

## Agent Instructions

1. Land the three tasks in order. Task 1 forces the IR shape the other two
   extend; do not start task 2 before task 1 compiles.
2. Write the IR change first, then fix the exhaustive-match sites until the
   tree builds. The compiler is the checklist.
3. Do not use `ast_edit` for the enum variants. A previous session tried it on
   a structurally similar change and the pattern matched a fraction of the
   sites, with a proposal that could not be inspected field by field.
4. Put a guard where it has the most context: a check needing a file and a
   line belongs in the parser, so every command rejects the input.
5. Reuse the parser's own helpers (`NamedChildren`, `node_text`, `line_of`,
   `parse_dto_class`, `parse_api_decorator`) instead of re-deriving syntax
   facts.
6. Keep the next free codes in mind and grep before you claim one: `E1016` is
   the next free parser code, `E2019` the next free generator code. `E1015` is
   already taken by the path-parameter work.
7. Keep modules under the 400-line ceiling, which already binds
   `transpiler/rust.rs` (561, grandfathered) and `parser/python.rs` (520,
   grandfathered). Split into a sibling module with a `tests.rs` first.
8. Move each finished tracker line to `tasks/completed.md` in the commit that
   finishes it.

## Output

A PR where a handler can loop, match, format a string, and use `//` and `%`
with Python's own semantics, on both targets — with each rejected construct
that remains carrying a diagnostic that names it and says what to write
instead.
