# Prompt 06: MCP Server and Agentic CLI

**Objective**: Give AI agents first-class access to Rivet. Ship an MCP
server that exposes the phase-1 and phase-2 surfaces (the parsed AST, the
MQI audit, and the `.rivet/` vector index), then add the agentic slash
commands `/plan`, `/fix`, and `/trace` on top of the same stores. Every
JSON diagnostic carries a `suggested_fix` so an agent can act without a
human.

**Context**: Phase 2 closed the context story. `rivet-cli` records every
command in a local SQLite store (`.rivet/rivet.db`), saves and resumes
compact markdown sessions, and indexes parsed blueprints in a local
LanceDB table (`.rivet/lancedb`) behind `store::vector`. `rivet explain`
already ties the nearest route chunk to the introducing commit via git
pickaxe. This phase puts that machinery behind two doors an agent can
walk through: the Model Context Protocol (MCP) and slash commands that
read as `/plan`, `/fix`, and `/trace`.

Two external reference points shape the `/plan` design:

- `github/spec-kit` (MIT) popularized spec-driven development: write an
  executable spec, plan it, implement it, then converge the code against
  the spec. Rivet does not embed spec-kit (it is Python); `/plan` mirrors
  its loop with Rivet-native artifacts.
- Users bring their own LLM provider. A provider is any OpenAI-compatible
  `/chat/completions` endpoint: a cloud vendor or a local model
  (ollama, LM Studio, vLLM). The provider key and endpoint come from the
  environment, never from the repo. Local providers need no key.

Token economy is a design constraint, not a feature. Paid providers bill
per token and subscription tiers cap usage, so `/plan` must spend tokens
like a budget: one compact deterministic context, structured JSON
responses, bounded retries, and local verification instead of asking the
model to check its own work. Rivet's own commands produce the evidence;
the model only writes code.

This phase serves `docs/pillars/09-super-cli.md`. The pillar is a stub
table today; finish it in the docs commit. SQLite, vector, and network
access stay in `rivet-cli` and never enter `rivet-core`.

---

## Tasks

### 1. Pin a maintained MCP server SDK

Research and pin the SDK that hosts the server. The candidates are the
official Rust SDK (`modelcontextprotocol/rust-sdk`) and `rmcp`. Neither is
a placeholder; pick the one that is maintained, resolves from the
registry, licenses cleanly under `deny.toml`, and compiles at the
workspace MSRV. Record the choice and the reason in
`docs/pillars/09-super-cli.md`.

- If the chosen SDK needs a newer rustc than the workspace MSRV, bump
  `rust-version` in the workspace manifest with the SDK version in the
  commit body. Do not bump without that justification.
- The server runs as a `rivet mcp` subcommand over stdio, the MCP
  transport agents expect.
- Acceptance: the crate resolves, and an in-crate protocol test drives a
  hello-world tool through `initialize`, `tools/list`, and `tools/call`
  and reads valid responses.

### 2. Expose the AST, audit, and vector index over MCP

Add `rivet-cli/src/mcp/` — one module per concern, each tool a thin
wrapper over an existing command or store function. Do not duplicate
logic; a tool never re-implements what `commands/` already does. Tool
arguments name the app file (for example `app.py`); the server resolves
the project store the way `commands::explain` does.

The tool set, mapped to what exists today:

- `parse_app` — parse the module and run the Verifier. Returns the
  `ServiceBlueprint` as JSON plus the findings (each with
  `suggested_fix`).
- `audit_app` — the MQI grade and dimension breakdown as JSON
  (`commands::audit`).
- `vector_search` — search the phase-2 blueprint index for a symptom and
  return the nearest route chunks with distances (`store::vector::search`).
- `explain_symptom` — the full `rivet explain` result: nearest route,
  introducing commit, fingerprint digest.
- `session_context` — the compact markdown context for the app
  (`commands::session::render_context`).
- `history` — the recorded command log (`store::list_commands`).

Build all JSON by hand from `serde_json::Value`; the `json!` macro is
banned. The server must stay small: transport and dispatch in one file,
the tools beside the code they wrap.

- Acceptance: an in-crate MCP protocol test lists the AST and vector
  tools and gets valid data for a fixture project.

### 3. Slash-command dispatch, `/trace`, and `/fix`

Agents type a leading slash, so `rivet /plan "..."` must work exactly as
written. Normalize a leading `/` on the first argument in `main()` before
clap parses, then dispatch to `commands::plan`, `commands::fix`, and
`commands::trace`. Each module returns `Result<(), Vec<Diagnostic>>` like
the existing commands and records its invocation in the store.

`rivet /trace "<symptom>"` traces a request through the system end to
end: parse the module, run the Verifier, embed the symptom and find the
nearest route chunk (vector), then report the route contract, the
generated Rust symbols the transpiler would emit, and the introducing
commit via git pickaxe. Print one trace with source spans and commit.

`rivet /fix` runs the Verifier and applies only fixes that are
deterministic and safe: deleting module items the rules flag as dead
(unused helpers and DTOs) or untranslatable (runtime classes, foreign
decorators, stray statements). Rewrite `app.py` by source span, then
re-parse and re-run the rules until the fixable findings are gone.
Unfixable findings stay, each carrying its `suggested_fix`. Never
"fix" a finding by weakening a rule or by deleting a route.

- Acceptance: each command has an in-crate integration test with a
  fixture project. `/fix` proves the fixture's dead and untranslatable
  items disappear and the module still parses and passes the rules that
  remain. `/trace` on a fixture symptom prints the route path, the
  generated symbols, and the introducing commit.

### 4. `/plan`: spec-driven code generation and auto-PR

`rivet /plan "<story>"` turns a user story into a working branch whose
tests pass, following a spec-kit-shaped loop: write the spec, generate
the code, converge the code against the spec.

The pipeline, in order:

1. **Context.** Parse and audit the current module. Assemble one compact
   deterministic context: the module summary, the `[verifier]` config,
   and the current findings. This context is the only project text the
   provider sees — no raw file dumps, no full transcripts.
2. **Spec.** Ask the provider for a spec document plus the full
   replacement `app.py` module that satisfies it, in one structured JSON
   response. The spec records the story, the routes and DTOs it adds, and
   its acceptance checks. The module must stay inside the documented DSL
   subset and tag every new route with the story IDs it serves.
3. **Converge.** Run the module through the real pipeline: parse, Verifier,
   `rivet build`. If a finding or a compile error appears, send the
   structured diagnostics back once (bounded retry, two attempts total)
   and converge again. Local verification is the arbiter; the model does
   not grade itself.
4. **Deliver.** Create branch `rivet/plan/<story-slug>` from HEAD, write
   the spec and the module, re-verify, and commit. Compose the PR body:
   the spec, the story, and the MQI audit grade of the resulting module.
   With `gh` available and authenticated, open the PR; otherwise print
   the branch and the push command.

The provider is any OpenAI-compatible endpoint. Configuration comes from
the environment, read at run time and never committed:

- `RIVET_PLAN_BASE_URL` — endpoint root (default `https://api.openai.com/v1`);
  point it at `http://localhost:11434/v1` for a local ollama.
- `RIVET_PLAN_API_KEY` — provider key; optional for local providers.
- `RIVET_PLAN_MODEL` — model name (default a small fast model).

Keep the provider behind one module (`commands::plan::provider`) with the
HTTP call, prompt assembly, and response parsing as separate small
functions. Parsing is pure and unit-testable offline from captured
responses; the HTTP call is not exercised by the gate.

`/plan` also accepts `--from <file>`: apply a prepared replacement module
as the plan's code and run steps 3 and 4. This deterministic path is the
one the gate exercises — no key, no network — and it is how a human or an
agent (for example one driving the MCP tools from task 2) lands a
hand-written change with full PR automation.

Token economy, encoded:

- The provider sees compact structured context, never raw tool spew.
- Responses are structured JSON parsed locally; a malformed response
  fails the attempt instead of being re-sent.
- The retry budget is two attempts. Verification output enters a retry as
  structured diagnostics, not as captured terminal text.
- The default model is small and fast; the model name is configurable.

- Acceptance: with `--from` and a fixture story on a git-initialized
  fixture project, `/plan` produces branch `rivet/plan/<slug>` whose
  module parses, passes the Verifier, builds, and answers a request for
  the new route. The prompt assembly and response parsing unit tests
  cover the provider module offline.

### 5. Agentic diagnostics: `suggested_fix` on every finding

Today `Diagnostic.suggested_fix` and `Finding.suggested_fix` are
`Option<String>` and most construction sites leave them `None`. An agent
cannot act on a bare error code. Make the fix mandatory: change the field
to `String` on both types, thread it through the constructors, and give
every construction site a concrete remediation written from that error
code's meaning. A generic "review and correct" is the last resort, never
the default. Keep the JSON payload shape stable — the field was already
serialized when present — but the payload now always carries it.

Where a code has a mechanical remediation (rename the identifier, add the
type hint, remove the dead helper, tag the route with `stories=[...]`),
say exactly that. Update the `json_payload_is_parseable_and_complete`
test and any other test that assumed an absent fix.

- Acceptance: a matrix of failing fixture modules (one per parser code
  and one per Verifier rule) plus the store and config error paths each
  emit a diagnostic whose JSON carries a non-empty `suggested_fix`.

### 6. Docs and tracker

As the features land, in the commit that lands each one:

- Extend `docs/pillars/09-super-cli.md` from the stub table to the real
  surface: the MCP tools and transport, the slash commands, the provider
  environment reference, the spec-kit lineage of `/plan`, and the token
  economy rules.
- Tick the phase-3 checkboxes in `docs/ROADMAP.md`.
- Move each finished `tasks/todo.md` line to `tasks/completed.md` with
  the finishing commit hash.

Acceptance Criteria
- The pinned MCP SDK resolves and a hello-world tool answers a probe
  request (`initialize`, `tools/list`, `tools/call`).
- An MCP client lists the AST, audit, and vector tools and gets valid
  data for a fixture project.
- `/plan`, `/fix`, and `/trace` each have an integration test on a
  fixture project.
- On a fixture story, `/plan --from <file>` produces a branch whose
  module passes the Verifier, builds, and answers a request for the new
  route.
- Every emitted diagnostic in an error scenario carries `suggested_fix`,
  Verifier findings included.
- The provider module reads base URL, key, and model from the
  environment and never commits them.
- `cargo fmt --all -- --check`, `cargo clippy -- -D warnings`, and
  `cargo test --workspace` all pass; `./scripts/gate.sh` is green.

Agent Instructions
1. Follow the command-module pattern: one small module per command in
   `rivet-cli/src/commands/`, each returning structured `Diagnostic`s.
2. Keep the MCP server small and thin: `rivet-cli/src/mcp/` wires
   transport to existing functions; it never grows new pipeline logic.
3. Store, vector, and provider network access live in `rivet-cli`, never
   in `rivet-core`.
4. New error codes stay in the ranges the tree already owns. The parser
   owns `E1xxx`, the generator and Verifier own `E2xxx`, and the context
   engine owns `E3xxx` — give the new commands the next free context
   codes after `E3010`.
5. Build JSON by hand from `serde_json::Value`. Never use the `json!`
   macro, `unwrap`, or `expect` outside tests.
6. Keep modules small — well under 400 lines including tests. Split a
   module that outgrows its concern instead of stretching it.
7. Reuse `render_context`, `store::vector`, and the Verifier exactly as
   they are. Do not invent a second convention beside an existing one.
8. No secrets, no placeholders, no invented facts. Provider keys exist
   only in the environment.
9. Move each finished tracker line to `tasks/completed.md` in the commit
   that finishes it, with the commit hash.

Output
A PR where an AI agent opens Rivet and finds its tools: an MCP server
exposing the AST, the audit, and the vector index; `/plan` that turns a
story into a verified branch with a spec and an audit grade; `/fix` that
repairs what the Verifier can repair; `/trace` that follows a request to
its introducing commit; and a `suggested_fix` on every error it emits —
all recorded in pillar 09 and the roadmap.
