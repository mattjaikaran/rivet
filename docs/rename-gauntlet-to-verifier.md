# Gauntlet to verifier rename inventory

Rivet's internal DSL-level gate is called "Gauntlet". A separate, standalone
Rust CLI named "Gauntlet" now exists as a language-agnostic, codebase-level
verification orchestrator. The two names collide, so Rivet's internal
component takes the name "Verifier". The standalone Gauntlet CLI keeps its
name.

This document inventories every file that contains the string `gauntlet`
(case-insensitive) before the rename. It lists the rename action per file.
The rename changes no behavior except the post-generation gate that
`rivet build` adds.

## Scope summary

| Group | Files |
| :--- | ---: |
| Rust source (`.rs`), inline test modules included | 30 |
| Config (`.toml`) | 2 |
| Docs (`docs/**/*.md`) | 11 |
| Prompts (`prompts/**/*.md`) | 5 |
| Tracker (`tasks/*.md`) | 2 |
| Other Markdown (`README.md`, `.github/CONTRIBUTING.md`) | 2 |
| CI workflow (`.github/workflows/ci.yml`) | 1 |
| **Total** | **53** |

The groups do not overlap and they sum to the total. The 30 Rust files
include every inline `#[cfg(test)]` module that mentions the old name; there
is no separate test-file count, because the matches live inside those same
files.

No Python source file (`.py`) contains the string. No standalone fixture file
contains the string either: the test matches are inline modules and fixture
constants, so they appear in the source-code table below. There is no shell
script and no Makefile target that names the component.

## Source code (`.rs`)

| File | What matches | Rename action |
| :--- | :--- | :--- |
| `rivet-cli/src/gauntlet/mod.rs` | module docs, `GauntletConfig`, `run_gauntlet` | move to `verifier/mod.rs`; rename identifiers |
| `rivet-cli/src/gauntlet/complexity.rs` | module docs, `crate::gauntlet`, test names | move to `verifier/complexity.rs` |
| `rivet-cli/src/gauntlet/dead_code.rs` | module docs, `crate::gauntlet` | move to `verifier/dead_code.rs` |
| `rivet-cli/src/gauntlet/duplicate.rs` | module docs, `crate::gauntlet` | move to `verifier/duplicate.rs` |
| `rivet-cli/src/gauntlet/story_link.rs` | module docs, `crate::gauntlet` | move to `verifier/story_link.rs` |
| `rivet-cli/src/gauntlet/type_strict.rs` | module docs, `crate::gauntlet` | move to `verifier/type_strict.rs` |
| `rivet-cli/src/main.rs` | `mod gauntlet;`, command help text | rename module declaration and prose |
| `rivet-cli/src/config.rs` | `GauntletConfig`, `RivetConfig::gauntlet`, `[gauntlet]` docs | rename type and field; add compat shim |
| `rivet-cli/src/diagnostic.rs` | module docs, error-code ranges | rename prose |
| `rivet-cli/src/commands/audit.rs` | `crate::gauntlet`, `GauntletConfig`, prose | rename imports, type, prose |
| `rivet-cli/src/commands/build.rs` | `crate::gauntlet`, prose, gate wiring | rename imports and prose; add post-generation gate |
| `rivet-cli/src/commands/explain.rs` | `crate::gauntlet`, printed strings | rename import and user-facing strings |
| `rivet-cli/src/commands/fix.rs` | `crate::gauntlet`, module docs, printed string | rename import, prose, user-facing string |
| `rivet-cli/src/commands/plan.rs` | `gauntlet_blockers` import, prose | rename import and prose |
| `rivet-cli/src/commands/session.rs` | `crate::gauntlet`, `## Gauntlet config` heading | rename import, field, heading |
| `rivet-cli/src/commands/trace.rs` | `crate::gauntlet`, printed strings | rename prose and printed strings |
| `rivet-cli/src/commands/plan/deliver.rs` | `crate::gauntlet`, `gauntlet_blockers` | rename import and function |
| `rivet-cli/src/commands/plan/provider.rs` | prompt string "pass the Gauntlet" | rename string |
| `rivet-cli/src/mcp/mod.rs` | module docs | rename prose |
| `rivet-cli/src/mcp/tools.rs` | tool descriptions, `crate::gauntlet` | rename descriptions and import |
| `rivet-cli/src/parser/python.rs` | doc comments on `ParsedModule` | rename prose |
| `rivet-cli/src/transpiler/rust.rs` | doc comments on `E2014` | rename prose |
| `rivet-core/src/ir/mod.rs` | doc comment on `stories` | rename prose |
| `constraint-tools/src/bin/check-file-length.rs` | `GAUNTLET_CEILING`, `GAUNTLET_PREFIX`, path prefix, test names | rename constants, path prefix, test names |
| `constraint-tools/src/bin/check-rule-modules.rs` | `check_gauntlet`, path prefix, test names | rename function, path prefix, test names |

## Config (`.toml`)

| File | What matches | Rename action |
| :--- | :--- | :--- |
| `rivet.toml` | `[gauntlet]` section and comment | rename section to `[verifier]`, update comment |
| `clippy.toml` | comment "Gauntlet-style limits" | rename prose |

## Docs (`docs/**/*.md`)

| File | What matches | Rename action |
| :--- | :--- | :--- |
| `docs/ROADMAP.md` | phase-1 title, bullets, status prose | retitle to "The Verifier"; add two-layer model |
| `docs/ARCHITECTURE.md` | pipeline diagram node, prose | rename node and prose |
| `docs/development-workflow.md` | gate diagram, self-check table, config example | rename prose and example section |
| `docs/nine-pillars.md` | pillar-7 name | rename pillar name |
| `docs/testing-strategy.md` | prose | rename prose |
| `docs/pillars/03-polyglot-frontend-support.md` | duplicate-rule prose | rename prose |
| `docs/pillars/04-persistent-context-engine.md` | `[gauntlet]` config reference | rename reference |
| `docs/pillars/05-story-to-code-traceability.md` | gate prose, `[gauntlet]` key | rename prose and key |
| `docs/pillars/06-matt-quality-index.md` | audit prose, `[gauntlet]` key | rename prose and key |
| `docs/pillars/07-the-gauntlet.md` | whole pillar | rename file to `07-the-verifier.md`; rename contents |
| `docs/pillars/09-super-cli.md` | `/fix` and `/plan` prose | rename prose |

## Prompts (`prompts/**/*.md`)

| File | What matches | Rename action |
| :--- | :--- | :--- |
| `prompts/prompt-01-ir.md` | pipeline prose | rename prose |
| `prompts/prompt-04-gauntlet.md` | whole prompt | rename file to `prompt-04-verifier.md`; rename contents; add migration note |
| `prompts/prompt-05-context.md` | pipeline prose | rename prose |
| `prompts/prompt-06-mcp.md` | tool and pipeline prose | rename prose |
| `prompts/prompt-07-ecosystem.md` | pipeline prose | rename prose |

## Tests and fixtures

No standalone test or fixture file contains the string at inventory time. The
matches live in inline test modules and in test-fixture source constants:

| File | What matches |
| :--- | :--- |
| `rivet-cli/src/config/tests.rs` | test names, `[gauntlet]` fixtures |
| `rivet-cli/src/commands/audit/tests.rs` | `GauntletConfig`, `crate::gauntlet` |
| `rivet-cli/src/commands/build/tests.rs` | test name, `config.gauntlet` |
| `rivet-cli/src/commands/build/tests/collisions.rs` | fixture doc comment |
| `rivet-cli/src/gauntlet/mod.rs` (tests) | `run_gauntlet`, `GauntletConfig` |
| `rivet-cli/src/gauntlet/complexity.rs` (tests) | `run_gauntlet`, `GauntletConfig` |
| `rivet-cli/src/transpiler/rust/tests/features.rs` | fixture doc comment |

## Shell scripts, Makefiles, CI, other

| File | What matches | Rename action |
| :--- | :--- | :--- |
| `.github/workflows/ci.yml` | job id `gauntlet-check`, job name, step name | rename job and step; rename the job key |
| `.github/CONTRIBUTING.md` | CI prose | rename prose |
| `README.md` | phase-1 prose | rename prose |
| `tasks/completed.md` | phase-1 history entries | rename prose except historical prompts |
| `tasks/todo.md` | phase table, phase-1 section | rename prose |

## Intentional references that stay

After the rename, these keep the word "Gauntlet":

- The deprecation shim for `[gauntlet]` in `rivet-cli/src/config.rs`, marked
  `// TODO(remove-in-0.2): remove [gauntlet] compat shim`.
- The standalone Gauntlet CLI references: the runtime `gauntlet` binary name,
  the `--no-gauntlet` flag, and the two-layer model notes in
  `docs/ROADMAP.md` and `prompts/prompt-04-verifier.md`.
- This inventory file.
