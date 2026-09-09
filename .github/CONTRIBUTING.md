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
   cargo deny check
   ./target/release/rivet audit --json examples/basic/app.py
   ./scripts/gate.sh
   ```
   `./scripts/gate.sh` runs fmt, clippy, tests, cargo-deny, the example
   build and audit, and the repository self-checks (`constraint-tools/`)
   in one pass. CI runs the same self-checks in the `repo-self-checks` job.
5. Open a PR against the main branch.

## Quality gates

CI enforces format, clippy (with `-D warnings`), tests, and cargo-deny CVE
scanning, and runs the Gauntlet on `examples/basic` through `rivet build`.
The `repo-self-checks` job runs the constraint tools that gate the Rivet
source tree itself: file-length ceilings, rule-module coherence, and
tracker coherence (see `docs/development-workflow.md`).

`rivet audit` reports the MQI grade for a DSL module: complexity,
duplicate-code, dead-code, and type-strictness dimensions fold into an A+
to F grade with a JSON breakdown (see `docs/pillars/06-matt-quality-index.md`).
Every Gauntlet failure prints machine-readable JSON diagnostics on stderr
that agents can act on.
