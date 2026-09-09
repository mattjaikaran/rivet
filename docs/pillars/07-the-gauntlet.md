# 7. The Gauntlet (Strict Linters)
The Gauntlet runs during transpilation (before code generation). It blocks bad code from ever reaching the Rust compiler.

Error Format (Agentic JSON):
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
