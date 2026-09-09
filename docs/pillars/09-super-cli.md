# 9. The Super-CLI
A CLI with slash-commands for AI agents:
| Command | Purpose |
| :--- | :--- |
| `rivet /plan` | AI agent generates code from a user story |
| `rivet /fix` | Auto-fixes Gauntlet errors |
| `rivet /trace` | Traces a request through the system |
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