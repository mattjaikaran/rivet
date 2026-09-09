# Contributing to Rivet

Thank you for considering contributing to Rivet. We build an API framework
that compiles a Python DSL into a fast Rust server, and we welcome help.

## Code of conduct

We adhere to the [Contributor Covenant](CODE_OF_CONDUCT.md). Read it before
contributing. Harassment or toxic behavior is not tolerated.

## How to contribute

### Report bugs

- Open an issue with the label `bug`.
- Provide a minimal reproduction case: a short code snippet that triggers the
  bug.
- Include:
  - Rust version (`rustc --version`)
  - Rivet version (`rivet --version`)
  - OS and architecture

### Suggest features

- Open an issue with the label `enhancement`.
- Describe the problem you are solving and why it matters.
- Propose an API design where relevant. We discuss and refine it before
  implementation.

### Submit pull requests

1. Fork the repository.
2. Create a branch: `git checkout -b feature/your-feature`.
3. Write code with tests.
4. Run the checks locally:
   ```bash
   cargo fmt --all -- --check
   cargo clippy -- -D warnings
   cargo test --workspace
   ```
5. Open a PR against the main branch.

## Quality gates

CI enforces format, clippy (with `-D warnings`), and tests today. The Gauntlet
(`rivet audit`) is a planned milestone that adds compile-time quality
enforcement on top: complexity limits, coverage floors, mutation testing, and
machine-readable JSON diagnostics for every failure.
