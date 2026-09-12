# Handoff: rename Gauntlet to Verifier

Branch `refactor/gauntlet-to-verifier`, seven commits on top of `5339cd3`
(local `main`, which is seven commits ahead of `origin/main`). Working tree
clean. Every commit compiles with `cargo check --workspace --all-targets`.

## What changed

Rivet's internal DSL-level gate is now **Verifier**. The standalone Rust CLI
named **Gauntlet** keeps its name; it is the codebase-level layer that runs
on the generated crate.

The rename covers code, config, docs, prompts, tests, and user-facing
strings. Error codes `E2042`-`E2046` are unchanged: they are stable
identifiers.

| Commit | Subject | Scope |
| :--- | :--- | :--- |
| `3e85f02` | `docs: Inventory gauntlet references for rename` | `docs/rename-gauntlet-to-verifier.md` |
| `a02c5ae` | `refactor: Rename the gauntlet module to verifier` | module move, types, identifiers, test names |
| `c06322a` | `refactor: Rename [gauntlet] config to [verifier]` | `[verifier]` plus the compat shim |
| `ef1d83b` | `docs: Rename gauntlet references to verifier` | docs, prompts, tracker, CI, README |
| `5d6361b` | `feat(build): Run gauntlet on the generated crate` | the post-generation step |
| `120eaed` | `test(build): Cover the post-generation gate` | the new integration tests |

## The code rename

- `rivet-cli/src/gauntlet/` moved to `rivet-cli/src/verifier/` (`git mv`, so
  history follows).
- `GauntletConfig` became `VerifierConfig`; `RivetConfig::gauntlet` became
  `RivetConfig::verifier`; `run_gauntlet` became `run_verifier`;
  `gauntlet_blockers` became `verifier_blockers`; `check_gauntlet` became
  `check_verifier`.
- The two `constraint-tools` binaries follow the new path:
  `GAUNTLET_CEILING`/`GAUNTLET_PREFIX` became `VERIFIER_*`, and the
  file-length ceiling now matches `rivet-cli/src/verifier/`.
- The CI job id `gauntlet-check` became `verifier-check`.

## Deprecation shims

Two shims exist. Both are marked for removal in `0.2`.

1. **`[gauntlet]` in `rivet.toml`** (`rivet-cli/src/config.rs`,
   `apply_deprecated_verifier_section`). A file that still carries the old
   section name loads, prints one line to stderr, and reads its values:

   ```
   [rivet] `rivet.toml` uses the deprecated `[gauntlet]` section; rename it to `[verifier]` (the old name is removed in 0.2)
   ```

   When both sections are present, `[verifier]` wins. The shim carries
   `// TODO(remove-in-0.2): remove [gauntlet] compat shim`.

2. **`rivet verifier` as a subcommand: this is a documented no-op.** There is
   no `rivet gauntlet` subcommand and no `rivet verifier` subcommand. The
   command set is `add`, `build`, `dev`, `audit`, `history`, `explain`,
   `plan`, `fix`, `trace`, `mcp`, `session`, and `sync`. `rivet audit` is
   already the command that runs the Verifier and reports the MQI grade, so
   the rename adds no subcommand and no hidden alias. The prompt's clause was
   conditional on an existing `rivet gauntlet` subcommand; the antecedent is
   false.

## The post-generation step

`rivet build` now runs the standalone Gauntlet CLI on the crate it just
wrote, before cargo compiles it. `rivet-cli/src/commands/build/gate.rs` owns
the step; `rivet-cli/src/commands/build.rs` and `build/wasm.rs` call it.

| `gauntlet check` exit code | `rivet build` behavior |
| :--- | :--- |
| `0` | Continue to compilation. |
| `2` | Abort before compiling; print the findings (`E2018`). |
| `3` | Print the findings and continue. |
| any other, or a signal | Abort (`E2018`). A broken gate must not look like a passing gate. |

- The CLI is invoked as
  `gauntlet check --tier=standard --target=<generated-crate>`.
- It is detected at runtime on `PATH` and is never a dependency.
- A host without it prints
  `[rivet] gauntlet CLI not found; skipping post-generation gate` once and
  continues.
- `rivet build --no-gauntlet` skips the step explicitly.
- **The step belongs to `rivet build` alone.** `rivet dev` and `rivet plan`
  call the same build path as a sub-step and pass the skip, because neither
  offers a `--no-gauntlet` escape and neither should carry a dependency the
  user did not aim at it. `rivet dev` serves the generated binary; `rivet
  plan` verifies through its own convergence loop.
- Both targets run it: native against `generated/`, WASI against
  `generated-wasm/`.

## Tests added

`rivet-cli/tests/post_generation_gate.rs` drives the real `rivet` binary with
a controlled `PATH`, so no network and no axum compile is involved:

- `build_aborts_on_gauntlet_blocker` — a stub `gauntlet` exits `2`; the build
  fails and a recording `cargo` stub proves compilation was never reached.
- `build_skips_gauntlet_when_absent` — `PATH` has no `gauntlet`; the build
  succeeds and prints the skip notice.
- `no_gauntlet_skips_the_step` — the flag skips a stub that would block.
- `a_deprecated_gauntlet_section_warns_and_still_builds` — the shim's
  user-facing half.
- `a_verifier_section_prints_no_deprecation_warning` — the current section
  name warns about nothing.

Unit tests in `rivet-cli/src/commands/build/gate.rs` cover the verdict
mapping, the `PATH` lookup, and the unknown exit code.

`rivet-cli/src/config/tests.rs` adds
`a_deprecated_gauntlet_section_still_loads` and
`a_verifier_section_wins_over_a_deprecated_gauntlet_section`.

The renamed test `verifier_gate_runs_before_codegen` (was
`gauntlet_blocker_stops_build_without_writing_a_crate`) proves a DSL-level
blocker prevents generation.

## Scope verification

A marker stub named `gauntlet` (exit `2`) on `PATH` proved the scope:

- `rivet build app.py` — the stub fires, the build exits non-zero, and stderr
  carries `blocking findings`.
- `rivet build --no-gauntlet app.py` — exits `0` and the stub is never
  invoked.
- `rivet plan "<story>" --from FILE app.py` on a clean repository — exits
  `0`, commits the branch, and the stub is never invoked.

`rivet dev` passes the same skip and was reviewed, not exercised: it starts a
long-running server.

### The test suite is independent of an installed binary

Every in-process `run_build` call in the test suites passes the skip, so a
developer who puts the Gauntlet CLI on `PATH` does not change what the suite
does. Proven by running the in-process build tests with a blocking stub
prepended to `PATH`:

```bash
printf '#!/bin/sh\nexit 2\n' > /tmp/poison/gauntlet && chmod 755 /tmp/poison/gauntlet
PATH="/tmp/poison:$PATH" cargo test -p rivet-cli --bin rivet -- commands::build::
```

All 24 tests pass. The real CLI path stays covered by
`rivet-cli/tests/post_generation_gate.rs`, which drives the actual `rivet`
binary with a controlled `PATH`.

## Intentional remaining references

`rg -i gauntlet` returns only:

- the `[gauntlet]` compat shim in `rivet-cli/src/config.rs`;
- the standalone CLI references: the runtime binary name, `--no-gauntlet`,
  the `gauntlet check` invocation, and the skip notice;
- the two-layer model docs in `docs/ROADMAP.md` and
  `prompts/prompt-04-verifier.md`;
- `docs/rename-gauntlet-to-verifier.md`, the inventory.

## Next steps

- **Rivet adapter for the Gauntlet CLI** (post-alpha). Centralize the
  DSL-level rules so they are versioned independently of Rivet.
- **Remove both shims in `0.2`**: the `[gauntlet]` config section and any
  remaining `TODO(remove-in-0.2)` markers.
- **Delete the inventory** when the shims go; it describes a rename that will
  then be complete.
