# 7. The Gauntlet (Strict Linters)

The Gauntlet runs between parse and code generation. It blocks bad code
from ever reaching the Rust compiler: a module that violates a blocker
rule fails `rivet build` with one agentic-JSON object per finding, and no
crate is written.

## Severity and outcome model

A finding is either a blocker or a warning:

- **Blocker** — stops the build. The CLI prints every blocker as JSON on
  stderr and exits non-zero.
- **Warning** — prints to stderr as JSON plus a one-line summary; the
  build continues.

The `[gauntlet]` section of `rivet.toml` tunes the rules:

```toml
[gauntlet]
max_complexity = 8          # E2042 threshold
stories_required = true     # set false to allow storyless routes
strict_type_checking = true # set false to skip the module-contract rule
duplicate_code = "blocker"  # outcome for E2043 (warn or block)
dead_code = "warning"       # outcome for E2044 (warn or block)
```

Rules that guard a hard guarantee (complexity, story links, type
strictness) are blockers when enabled.

## Rules and error codes

| Code | Rule | Blocks |
| :--- | :--- | :--- |
| E2042 | Cyclomatic complexity | Handler or DTO body over `max_complexity` |
| E2043 | Duplicate code | Two or more handlers sharing an implementation |
| E2044 | Dead code | Helper or DTO nothing references |
| E2045 | Story-to-code gate | Route without a story ID |
| E2046 | Type strictness | Module construct the engine cannot translate |

Each rule owns a small module under `rivet-cli/src/gauntlet/` and documents
its metric and contract there (see `complexity.rs` for the decision-point
list and `type_strict.rs` for the accepted dynamic shapes).

## Error Format (Agentic JSON)

A complexity finding:

```json
{
  "error_code": "E2042",
  "severity": "blocker",
  "file": "src/domains/payment/handler.py",
  "line": 42,
  "column": 8,
  "message": "Cyclomatic complexity exceeded (12 > max 8).",
  "suggested_fix": "Extract the 'validate_credit_card' logic into 3 separate strategy classes: 1. ValidateFormat 2. ValidateLuhn 3. ValidateExpiry",
  "ast_path": "payments.validate_credit_card.if_statements.branch_3"
}
```

`ast_path` names the declaration and, for complexity, the decision that
crossed the limit (for example `create_order.if_statement.2`), so agents
can point a fix at the exact branch. Every finding carries a
`suggested_fix`.

## Phase-1 status

The five rules above are enforced by `rivet build`. `rivet audit` (the MQI
grade over these findings) and the CI `gauntlet-check` job that runs the
example through the rules land with the audit and CI tasks; coverage and
mutation-survival numbers stay Rust-side until the generated-code test
story exists, as recorded in the MQI pillar
(`docs/pillars/06-matt-quality-index.md`).
