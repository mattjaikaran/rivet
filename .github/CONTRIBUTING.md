
# Contributing to Rivet

First off, thank you for considering contributing to Rivet! We're building the future of API frameworks, and we need your help.

## Code of Conduct

We adhere to the [Contributor Covenant](CODE_OF_CONDUCT.md). Please read it before contributing. Harassment or toxic behavior will not be tolerated.

---

## How to Contribute

### Reporting Bugs

- Open an issue with the label `bug`.
- Provide a **minimal reproduction case** (a short code snippet that triggers the bug).
- Include:
  - Rust version (`rustc --version`)
  - Rivet version (`rivet --version`)
  - OS and architecture

### Suggesting Features

- Open an issue with the label `enhancement`.
- Describe the problem you're solving and why it matters.
- Provide a proposed API design (if applicable).
- We'll discuss and refine it before implementation.

### Submitting Pull Requests

1. **Fork** the repository.
2. **Create a branch**: `git checkout -b feature/your-feature` or `fix/your-bug`.
3. **Write code** with tests.
4. **Run the Gauntlet locally**:
   ```bash
   rivet audit --min-grade A
   ```
   If it fails, the CLI will output a JSON error with a suggested fix. Follow it. If you don't understand the error, ask for help in the PR.
5. **Ensure CI passes** (GitHub Actions will run the Gauntlet automatically).
6. **Open a PR** against the main branch. Fill out the template.


The Gauntlet (Quality Gates)
All PRs must pass these metrics. The CI will block merging if any fail.

| Metric | Threshold | How it's enforced |
| :--- | :--- | :--- |
| Cyclomatic Complexity | < 8 per function | AST walker |
| Test Coverage | ≥ 80% | cargo tarpaulin |
| Mutation Survival | 0% | cargo-mutants |
| Dead Code | 0% | Rust dead_code lint |
| Redundant Code | 0% | AST duplication detection |
| Type Strictness | 0 any/unknown | DSL parser rejects dynamic types |

If your PR fails, the CI will output a JSON error with:

- `error_code`
- `file_path`
- `line/column`
- `message`
- `suggested_fix`
- `ast_path` (for pinpointing)

You can run `rivet fix` to attempt an automatic remediation.
