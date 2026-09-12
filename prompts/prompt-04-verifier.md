# Prompt 04: The Verifier (Compile-Time Quality Gates)

> Note: Rivet's DSL-level gate was previously called "Gauntlet". It is now
> called "Verifier" to avoid confusion with the standalone Gauntlet CLI, a
> language-agnostic codebase-level verification tool that runs on the
> generated Rust crate.

**Objective**: Run strict quality rules during transpilation, before code
generation, and fail `rivet build` with machine-readable JSON when a module
violates them. Report a quality grade through `rivet audit`.

**Context**: The Verifier is the "always-on reviewer": a set of small rule
modules under `rivet-cli/src/verifier/` that inspect the parsed module and
emit findings in the agentic-JSON diagnostic shape defined in
`docs/pillars/07-the-verifier.md`. Blocker findings stop the build before any
crate is written; warnings report to stderr and let the build continue.
Rules are configurable through the `[verifier]` section of `rivet.toml`.
The phase serves pillars 05 (story-to-code traceability), 06 (the MQI), and
07 (the Verifier error contract).

---

## Tasks

### 1. Extend the severity model

`rivet-cli/src/diagnostic.rs` defines `Severity` with a single `Blocker`
variant and documents that the Verifier extends it. Add `Warning` and a
`Diagnostic::warning` constructor. Accept the words `warn`/`warning` and
`block`/`blocker` when `Severity` is deserialized from `rivet.toml`.

### 2. Parse the `[verifier]` config

Extend `RivetConfig` in `rivet-cli/src/config.rs` with a `verifier` field:

| key | type | default | meaning |
| :--- | :--- | :--- | :--- |
| `max_complexity` | int | 8 | maximum cyclomatic complexity |
| `stories_required` | bool | true | require a story ID on every route |
| `strict_type_checking` | bool | true | run the type-strictness rule |
| `duplicate_code` | severity | `blocker` | outcome for duplicate handlers |
| `dead_code` | severity | `warning` | outcome for unused helpers and DTOs |

Unknown keys inside `[verifier]` stay ignored so the file can grow ahead of
the engine.

### 3. Keep the module's syntax tree after parsing

The Verifier rules need the tree-sitter tree, which the parser currently
drops. In `rivet-cli/src/parser/python.rs` introduce a `ParsedModule` that
owns the source text, the tree, the blueprint, and a declaration table for
top-level module items:

- `Route` (an `api`-decorated function)
- `Helper` (an undecorated function)
- `Foreign` (a decorated function without an `api` decorator)
- `Dto` (an annotation-only class)
- `RuntimeClass` (any other class)
- `Other` (module-level statements that are not imports or a docstring)

`parse_python_file` returns the module; keep `parse_python_source` returning
a bare `ServiceBlueprint` so existing tests keep compiling.

### 4. Build the Verifier harness

Create `rivet-cli/src/verifier/mod.rs` and one small module per rule beside
it (`complexity.rs`, `duplicate.rs`, `dead_code.rs`, `story_link.rs`,
`type_strict.rs`), mirroring the parser layout. The harness owns:

- a `Rule` trait: `id()`, `default_severity()`, and `check(ctx)` returning
  findings;
- a `Finding` type that converts into the `Diagnostic` JSON shape;
- a severity policy: the config decides which rules run and at which
  severity (see the table in task 2; `complexity`, `story_link`, and
  `type_strictness` are always blockers when enabled);
- `run_verifier(module, config) -> Vec<Diagnostic>`, with rule registrations
  in one place so a future rule slots in by adding a module and a line.

### 5. Wire the Verifier between parse and generate

In `rivet-cli/src/commands/build.rs`, run the Verifier after parsing and
before generating. Warnings print to stderr as JSON plus a one-line summary;
blockers return from the command so the CLI exits non-zero with the same
output and no crate is written. Change the build command's error type to
carry more than one diagnostic so all findings reach the agent at once.

### 6. Rules

Implement these rules. Each one owns an error code in the `E204x` range and
documents its contract in its module doc comment.

- **Complexity walker** (`E2042`, `[verifier] max_complexity`): count
  decision points (`if`, `elif`, `for`, `while`, `and`/`or`, ternary
  conditionals, `match` arms) in route handler bodies and DTO classes. Base
  score is 1. Fail with an `ast_path` naming the decision that crossed the
  limit, e.g. `module.create_order.if_statement.2`.
- **Duplicate-code detector** (`E2043`): hash each handler's implementation
  (parameter list, return annotation, and body text). Routes that share an
  implementation produce one finding naming every location.
- **Dead-code rule** (`E2044`): flag helper functions nothing references and
  DTO classes no route uses. Reference means a call from another module
  function or a route request/response type.
- **Story-to-code gate** (`E2045`): every route carries at least one story
  ID unless `stories_required = false`. Suggest the missing decorator
  argument as the fix.
- **Type-strictness rule** (`E2046`): reject module constructs that cannot
  reach the generated server: runtime classes (classes with methods or
  values), decorated functions without an `api` decorator, non-`api`
  decorators stacked on a handler, and module-level statements other than
  imports and the docstring. List every accepted dynamic shape (for example
  `dict` request bodies, `-> dict` responses, `dict` and `list` DTO fields)
  in the rule doc: dynamic typing is allowed only at the documented JSON
  boundary.

### 7. The MQI subset in `rivet audit`

Add an `audit` subcommand that runs the rules and prints a grade plus a
JSON breakdown per `docs/pillars/06-matt-quality-index.md`, phase-1 subset:
complexity (High weight), duplicate code, dead code, and type strictness.
Decide and record how test coverage and mutation survival enter the index
before the phase closes; they may stay Rust-side until the generated-code
test story exists.

### 8. Example and CI

- Give every route in `examples/basic/app.py` a story ID so the example
  passes the default gate.
- Replace the placeholder step in the `verifier-check` GitHub Actions job
  with a real `rivet build` on `examples/basic` that fails on rule
  violations with JSON visible in the log.
- Add a `cargo-deny` CVE-scanning job.

Acceptance Criteria
- A module that violates a rule fails `rivet build` with one JSON object per
  finding on stderr and no crate written.
- A handler with cyclomatic complexity above `max_complexity` fails with
  `E2042` and an `ast_path`.
- Two byte-identical handlers produce one finding naming both locations.
- An unused helper or DTO triggers the configured outcome (`dead_code`).
- A route without `stories=[...]` fails with a fix suggestion; the example
  passes the default gate.
- Every accepted dynamic type shape is listed in the type-strictness rule
  doc; anything outside it fails with `E2046`.
- `rivet audit` prints a grade and a parseable breakdown.
- `cargo fmt --all -- --check`, `cargo clippy -- -D warnings`, and
  `cargo test --workspace` all pass.

Agent Instructions
1. Extend the severity model first, then the config, then the parser
   module, then the harness. Rules come last; each is a small module with
   unit tests.
2. Reuse the parser's own helpers (`NamedChildren`, `node_text`, `line_of`,
   `parse_dto_class`, `parse_api_decorator`) instead of re-deriving syntax
   facts in the Verifier.
3. Keep rule modules under 300 lines including tests.
4. Move each finished tracker line to `tasks/completed.md` in the commit
   that finishes it.

Output
A PR where `rivet build` enforces the Verifier rules at compile time, an
example with a missing story ID or an over-complex handler fails with
agentic JSON, and `rivet audit` reports the phase-1 quality grade.

---

## Rename and migration

This prompt named the component "Gauntlet". Two names now exist, so read them
as separate layers:

- **Verifier** is Rivet's own DSL-level gate, described above. It runs inside
  `rivet build` between parse and code generation.
- **Gauntlet** is a standalone Rust CLI: a language-agnostic, codebase-level
  verification orchestrator. `rivet build` runs it on the generated crate
  after generation and before compilation.

### Config migration

The `[gauntlet]` section of `rivet.toml` is now `[verifier]`. The old name
still loads for this release: the loader prints
`[rivet] 'rivet.toml' uses the deprecated '[gauntlet]' section; rename it to
'[verifier]' (the old name is removed in 0.2)` to stderr and reads its values.
When both sections are present, `[verifier]` wins. The shim carries
`// TODO(remove-in-0.2): remove [gauntlet] compat shim`.

Error codes `E2042`-`E2046` do not change; they are stable identifiers.

### The post-generation step

`rivet build` writes the generated crate to disk, then runs
`gauntlet check --tier=standard --target=<generated-crate>` when the
`gauntlet` binary is on `PATH`, and before cargo compiles the crate:

| `gauntlet check` exit code | `rivet build` behavior |
| :--- | :--- |
| `0` | Continue to compilation. |
| `2` | Abort the build, print the findings, compile nothing (`E2018`). |
| `3` | Print the findings and continue to compilation. |
| any other | Abort: a broken gate must not look like a passing gate (`E2018`). |

When the binary is absent, the build prints
`[rivet] gauntlet CLI not found; skipping post-generation gate` once and
continues. `rivet build --no-gauntlet` skips the step explicitly. The binary
is detected at runtime only; Rivet never depends on it.

Only the `build` subcommand runs this step. `rivet dev` and `rivet plan` call
the same build path as a sub-step and skip it, because neither command offers
a `--no-gauntlet` escape and neither should carry a dependency the user did
not aim at it. `rivet dev` serves the generated binary, and `rivet plan`
verifies through its own convergence loop.

The step applies to both targets. `rivet build --target wasm` runs it against
`generated-wasm/` on the same terms.
