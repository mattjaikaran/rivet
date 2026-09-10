# 9. The Super-CLI

A CLI with slash-commands for AI agents. An agent types a leading slash;
the CLI strips it before clap parses, so `rivet /plan "..."` works exactly
as written.

| Command | Purpose |
| :--- | :--- |
| `rivet /plan "<story>"` | Turns a user story into a verified `rivet/plan/*` branch with a spec and an audit grade |
| `rivet /fix` | Applies the Gauntlet's deterministic repairs |
| `rivet /trace "<symptom>"` | Traces a request from the DSL route through the generated code to its introducing commit |
| `rivet session save` | Saves current context (with compacted Markdown) |
| `rivet session resume` | Restores a session |
| `rivet explain` | Explains a bug using vector search |
| `rivet audit` | Runs the MQI and outputs the grade |

## MCP server

`rivet mcp` serves the MCP tools over stdio for AI agents.

The SDK is `rmcp` 3.x, the official Rust SDK for the Model Context
Protocol (`modelcontextprotocol/rust-sdk`). It is Apache-2.0, actively
released, and its `rust-version` (1.88) sits below the workspace MSRV
(1.91), so pinning it needed no `rust-version` bump. The workspace pins
the `server`, `macros`, and `transport-io` features; the server runs the
`#[tool_router]` service over `rmcp::transport::stdio`.

Tool parameters declare their JSON schema by hand (a flat object of
string fields): the schemars derive macro expands to generated `unwrap`
calls, which the repo's clippy policy bans, and lint allows on a struct
do not reach macro-expanded code.

Every tool is a thin wrapper over an existing CLI function; a tool never
re-implements pipeline logic. The tool set maps onto the shipped
machinery:

- `parse_app` parses a DSL module and returns its IR blueprint and
  Gauntlet findings as JSON.
- `audit_app` returns the MQI grade and dimension breakdown.
- `vector_search` embeds a symptom and returns the nearest blueprint
  route chunks with distances.
- `explain_symptom` traces a symptom to its nearest route, introducing
  commit, and module digest.
- `session_context` renders the compact markdown module context.
- `history` lists the recorded CLI invocations for the project.

The server runs on the same current-thread tokio runtime the CLI uses
elsewhere; tools that touch the vector store are async methods so the
server never nests runtimes.

## Slash commands

`/fix` re-runs the Gauntlet and applies only deterministic, safe repairs
by source span: dead helpers and DTOs (E2044) and runtime classes,
foreign functions, and stray module statements (E2046). It converges in
up to five parse/re-check rounds and never deletes a route. Findings the
CLI cannot repair safely stay, each carrying its `suggested_fix`.

`/trace` embeds the symptom, finds the nearest route chunk in the vector
index, then follows the route through the pipeline: the DSL source line,
the axum registration and handler the transpiler would render, and the
commit that introduced the handler (git pickaxe).

`/plan` follows a spec-driven loop modeled on `github/spec-kit` (MIT;
Rivet does not embed it, it mirrors the workflow with Rivet-native
artifacts): write the spec, generate the code, converge the code against
the spec, deliver a branch.

1. **Spec.** The provider returns a spec document plus the full
   replacement `app.py` in one structured JSON response. New routes must
   tag the story IDs they serve and stay inside the documented DSL
   subset.
2. **Converge.** The module runs through the real pipeline: parse, the
   Gauntlet, and `rivet build`. Local verification is the arbiter; the
   model does not grade itself. On failure the structured diagnostics
   go back to the provider once (two attempts total).
3. **Deliver.** Branch `rivet/plan/<slug>` from HEAD, SPEC.md and the
   module committed, PR body composed from the story, the diff, and the
   audit grade. `--push` opens the PR through `gh`; otherwise the command
   prints the push command.

### Providers and environment

The provider is any OpenAI-compatible `/chat/completions` endpoint: a
cloud vendor or a local model (ollama, LM Studio, vLLM). Configuration
comes from the environment and is never committed:

- `RIVET_PLAN_BASE_URL` — endpoint root (default
  `https://api.openai.com/v1`); point it at `http://localhost:11434/v1`
  for a local ollama.
- `RIVET_PLAN_API_KEY` — provider key; required only when the base URL is
  unset, optional for local providers.
- `RIVET_PLAN_MODEL` — model name (default `gpt-4o-mini`).

`/plan --from <file>` applies a prepared replacement module and runs the
same converge-and-deliver pipeline. It needs no key and no network, which
is the path the gate exercises.

### Token economy

Paid providers bill per token and subscription tiers cap usage, so
`/plan` spends tokens like a budget:

- The provider sees one compact context (`session_context` markdown),
  never raw file dumps.
- Responses are structured JSON, parsed locally; malformed output fails
  the attempt instead of being re-sent.
- The retry budget is two attempts; verification output re-enters a
  retry as structured diagnostics, not captured terminal text.
- The default model is small and fast; the model name is configurable.

## Agentic diagnostics

Every diagnostic carries a `suggested_fix`. The field is a required
`String` on both `Diagnostic` and `Finding`; the JSON payload always
emits it, and each construction site supplies a remediation written from
its error code's meaning. A parser error says which annotation to add; a
Gauntlet finding says which helper to remove or how to tag a route; an
internal failure says what to check before reporting. An agent receives
no bare error code.
