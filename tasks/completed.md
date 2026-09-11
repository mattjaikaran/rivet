# Completed tasks

Tasks are moved here from `tasks/todo.md` when their definition of done is
met. Each entry notes the date and the commit that finished it.

## Phase 0 - The Transpiler Spike (complete 2026-09-09)

Served by `prompts/prompt-00-scaffold.md` through `prompts/prompt-03-generator.md`.
Commits: `dd8920f` (foundation) and `77fe374` (feature).

### Foundation and repo hygiene

- Reset workspace and member manifests to the dependency set each phase needs;
  removed duplicated `rivet-cli` manifest and aspirational pins
  (`dd8920f`).
- Replaced placeholder metadata: repository now `mattjaikaran/rivet`, authors
  set, no invented emails anywhere in docs or security files (`dd8920f`).
- Rewrote Dockerfile (CLI-only multi-stage), docker-compose (Postgres + Redis
  dev profile), and Makefile; removed emoji shell output and the unused
  entrypoint script (`dd8920f`).
- Cleaned `.github` docs (CONTRIBUTING, SECURITY, CODE_OF_CONDUCT) and made CI
  wording honest about planned gates (`dd8920f`).
- Aligned clippy config with the current schema (`too-many-*` kebab-case) and
  removed unstable rustfmt options (`dd8920f`).
- Wrote README, `rivet.toml`, `.dockerignore`, and updated gitignore so nested
  `generated/` output stays out of git (`dd8920f`).
- Rebased docs to reality: ROADMAP phase-0 checkboxes, phase-0 spike status,
  testing-strategy cleanup (`dd8920f`).

### Prompt 01 - IR (rivet-core)

- `rivet-core/src/ir.rs`: `HttpMethod`, `TypeRef`, `FieldDefinition`,
  `StructDefinition`, `RequestSpec`, `ResponseSpec`, `Expr`, `RouteDefinition`,
  `ServiceBlueprint`; serde round-trip tests (`77fe374`).

### Prompt 02 - Python parser (rivet-cli)

- Modular parser under `rivet-cli/src/parser/`: `decorator`, `signature`,
  `body`, `expr`, `types`, `validate`, orchestrated by `python.rs`
  (`77fe374`).
- tree-sitter 0.27 / tree-sitter-python 0.25 based parsing of `@api.<method>`
  decorators, typed signatures, annotation-only DTO classes, and a documented
  handler-body expression subset with string-literal decoding (`77fe374`).
- Structured diagnostics E1001-E1011 emitted as machine-readable JSON
  (`77fe374`).

### Prompt 03 - Rust generator

- `rivet-cli/src/transpiler/rust.rs` renders DTO structs, axum handlers, the
  router, and an isolated `Cargo.toml` (`77fe374`).
- `rivet build` reads `rivet.toml`, writes the crate to `generated/`, and
  compiles it (`77fe374`).

### Verification

- 30 tests green across the workspace; `cargo clippy -- -D warnings` clean;
  `cargo fmt --check` clean.
- Live spike verified from the compiled binary: `GET /ping` returns
  `{"status":"pong"}` and `POST /echo` echoes the JSON body.

## Phase 1 - The Gauntlet (complete 2026-09-09)

Served by `prompts/prompt-04-gauntlet.md`. All four sections are done;
see the per-section entries below. The mutation-tester roadmap bullet
stays open (blocked on the generated-code test story).

### 1.1 Foundation

- Authored `prompts/prompt-04-gauntlet.md` in the prompts 00-03 format;
  every 1.1-1.4 task traces to it (`3e09b54`).
- Defined the Gauntlet rule interface under `rivet-cli/src/gauntlet/`: one
  module per rule, a severity model (blocker/warning), a `Rule` trait, and
  findings that serialize to the agentic-JSON diagnostic shape; unit tests
  cover the harness (`3e09b54`).
- Wired the Gauntlet between parse and generate in `commands/build.rs`;
  blockers return on the `Diagnostic` JSON path with no crate written, and
  warnings print while the build continues (`3e09b54`).

### 1.2 Rules

- Cyclomatic complexity walker over DSL handler bodies and DTO classes,
  failing above `rivet.toml` `[gauntlet] max_complexity`: a synthetic
  handler with score 10 fails with `E2042` and an `ast_path` naming the
  crossing decision; the `examples/basic` handlers pass (`3e09b54`).
- Duplicate-code detector over handler implementation fingerprints: two
  byte-identical handlers produce one `E2043` finding naming both
  locations and the build blocks by severity (`3e09b54`).
- Dead-code rule for helpers and DTOs: an unused helper triggers the
  configured outcome (`E2044`, warning by default, `block` overridable)
  and tests lock the behavior (`3e09b54`).
- Story-to-code gate (pillar 05): a route without `stories=[...]` fails
  with `E2045` and a fix suggestion; the example passes the default gate
  (`3e09b54`).
- Type-strictness rule closing the module-level gaps the parser leaves to
  the IR: runtime classes, foreign decorators, and stray statements fail
  with `E2046`; the accepted dynamic shapes are listed in the rule doc
  (`3e09b54`).

### Verification

- 62 tests green; `cargo clippy -- -D warnings` and `cargo fmt --check`
  clean (`3e09b54`).
- End to end: `rivet build examples/basic/app.py` passes the gate, the
  binary answers `GET /ping` and `POST /echo`, a storyless route exits 1
  with JSON on stderr and no crate written, and a dead-code warning prints
  JSON while the build succeeds (`3e09b54`).

### 1.3 The MQI

- Implemented `rivet audit` (grade plus JSON breakdown) per pillar 06:
  complexity, duplicate code, dead code, and type strictness carry the
  pillar weights into an A+ to F grade; a blocker finding deducts 20
  points and a warning 10. Unit tests cover the aggregation and the
  grade bands (`55e599d`).
- Decided and documented how coverage and mutation survival enter the
  MQI: they stay Rust-side until the generated-code test story exists
  and appear in the `rivet audit` `not_scored` JSON list with their
  reasons; written into pillar 06 and the audit module doc (`b0c96c8`).

### 1.4 Integration and docs

- The CI `gauntlet-check` job runs the Gauntlet on `examples/basic`: a
  real `rivet build` plus `rivet audit --json`, replacing the
  placeholder `--version` step (`ce70633`).
- Added cargo-deny CVE scanning: `deny.toml` and a CI job that runs
  `cargo deny check`; verified locally that the check fails on
  `RUSTSEC-2020-0071` (`ce70633`).
- Ticked the phase-1 ROADMAP checkboxes, documented the real commands
  in `docs/development-workflow.md`, and aligned `CONTRIBUTING.md`,
  pillar 07, and the README with `rivet audit` (`99f086e`).
- Authored `prompts/prompt-05-context.md` for phase 2 in the prompts
  00-04 format (`24be798`).
- Closed phase 1 in the tracker and roadmap; the mutation-tester
  roadmap bullet stays open, blocked on the generated-code test story
  (see pillar 06, `not_scored`).

## Constraint tools (complete 2026-09-09)

Not a roadmap phase: deterministic self-checks gate the Rivet source tree
the way the Gauntlet gates DSL apps, preceding phase 2 so later work
inherits them. Served by the SwarmForge constraint-tools pattern in
`~/dev/django-ninja-boilerplate/docs/CONSTRAINT_TOOLS.md`.

- Added the `constraint-tools` workspace crate with three small
  deterministic binaries: `check-file-length` (400-line default ceiling,
  300 for Gauntlet rule modules, four grandfathered legacy files frozen
  at 728/699/679/444), `check-rule-modules` (error-code table, `pub mod`
  declarations, rule files, and doc codes agree 1:1), and `check-tracker`
  (no `- [x]` left in todo.md, no duplicate or cross-file task text, no
  open item under a `(complete ...)` section); each exits 0 or 1
  (`cb3e3bf`).
- One gate command: `scripts/gate.sh` runs fmt, clippy (`-D warnings`),
  tests, `cargo deny check`, the example build and audit, and the three
  self-checks in order, stopping at the first failure with its output
  visible; Makefile targets `gate` and `self-check` delegate to it
  (`86d8803`).
- CI wiring: the `repo-self-checks` job builds and runs the three
  self-check binaries on every push to main and PR, so a planted
  violation fails the job with the violation in the log (`4162a09`).
- Self-enforcement and docs: the tools gate themselves (the crate is
  inside the file-length and rule-module checks and passes), and the real
  commands, thresholds, and how-to-add-a-check steps are recorded in
  `docs/development-workflow.md`, `CONTRIBUTING.md`, and the README
  layout table (`25ed04e`).

### Verification

- 93 tests green (20 new), fmt and clippy clean, gate passes end to end.
- A planted over-long rule module (`complexity.rs` at 309 lines) fails
  `check-file-length`, an undocumented rule module fails
  `check-rule-modules`, a leftover `- [x]` line fails `check-tracker`,
  and the full gate exits 1 with the violation visible.

## Phase 2 - Context engine (complete 2026-09-09)

Served by `prompts/prompt-05-context.md`. The `.rivet/` store persists
command history, sessions, and fingerprints next to the app module; a
LanceDB vector index answers `rivet explain`. Decisions recorded in
`docs/pillars/04-persistent-context-engine.md`.

### 2.1 Local store

- `.rivet/` layout and SQLite schema (`commands`, `sessions`,
  `fingerprints`) behind `rivet-cli/src/store/`; rusqlite (bundled) chosen
  for portability and recorded in pillar 04; idempotent migration via
  `PRAGMA user_version` (`617632e`).
- `rivet history` lists recorded commands newest-first with exit status
  and duration; invocations are recorded before they run and finished with
  their status and duration (`617632e`, `68e93f9`).
- `rivet session save` renders the current module context (parsed
  blueprint, gauntlet config, diagnostics) as compact markdown and stores
  it; `session resume` prints it back; `session list` names the saved
  sessions (`617632e`).

### 2.2 Semantic search

- LanceDB behind `store::vector`: blueprint routes become deterministic
  character-trigram chunks in `.rivet/lancedb`; an async test indexes two
  routes and ranks the orders route first for an orders symptom
  (`8dd08e7`).
- `rivet explain "<symptom>"` embeds the symptom, finds the nearest route,
  and reports the introducing commit via git pickaxe on the handler; the
  module digest is stored per commit (`8dd08e7`).

### 2.3 Docs

- Pillar 04 rewritten with the store layout, crate choices (rusqlite
  bundled; lancedb 0.38 requiring the `remote` feature), and the commands;
  phase-2 ROADMAP checkboxes ticked; tracker section closed in this commit.

### Verification

- 106 tests green; fmt, clippy (`-D warnings`), and `cargo deny check`
  clean.
- End to end: two builds and a failing build appear in `rivet history`
  with correct statuses; a session round-trips save -> resume; on a
  two-commit fixture `rivet explain "orders failing"` names the commit
  that added the orders route.

## Phase 3 - MCP and agentic CLI

Served by `prompts/prompt-06-mcp.md`. Entries land here as their tracker
lines finish; the section closes when the phase does.

### 1.1 SDK pin

- Pinned `rmcp` 3.2 as the MCP server SDK: it is the official Rust SDK
  for the Model Context Protocol (`modelcontextprotocol/rust-sdk`,
  Apache-2.0), actively released, and compiles at the workspace MSRV (its
  `rust-version` is 1.88, below the workspace 1.91), so no `rust-version`
  bump was needed. Added to the workspace manifest with the `server`,
  `macros`, and `transport-io` features; the choice is recorded in pillar
  09. Tool parameters declare their JSON schema by hand because the
  schemars derive expands to banned `unwrap` calls (`21e2cb6`).
- `rivet mcp` serves the tool router over the stdio transport; an
  in-crate protocol test drives the real server over an in-memory duplex
  through `initialize`, `tools/list`, and `tools/call` and reads valid
  responses. The first tool, `parse_app`, returns the IR blueprint and
  Gauntlet findings for a DSL module as JSON (`21e2cb6`).

### 1.2 MCP tool set

- Completed the `rivet mcp` tool set, each tool a thin wrapper over an
  existing command or store function: `audit_app` (via the new
  `audit_json` seam), `vector_search` and `explain_symptom` (over the
  phase-2 LanceDB index), `session_context`, and `history`. The protocol
  probe test lists all six tools and calls two over the wire; unit tests
  cover every payload (`5642450`).
- Split `rivet explain` into an async core (`explain_async`) plus a sync
  runtime wrapper (`explain_data`) so the MCP server can await the vector
  store without nesting tokio runtimes, and shared `route_summaries`
  between the command and the tools (`5642450`).

### 1.3 Slash commands

- `rivet /plan`, `/fix`, and `/trace` dispatch through clap subcommands;
  main strips a leading `/` from the subcommand token so agents can type
  the slash form exactly. Each command records in the store and returns
  structured diagnostics (`59ffc8a`).
- `/trace "<symptom>"` follows a symptom from the matched DSL route
  (phase-2 vector index) through the axum code `generate_project` would
  render (registration line and handler signature) to the introducing
  commit via git pickaxe. Integration test on a two-commit git fixture
  (`59ffc8a`).
- `/fix` re-runs the Gauntlet and applies only deterministic, safe
  repairs by source span: E2044 dead helpers and DTOs, and E2046 runtime
  classes, foreign functions, and stray statements. It converges in up to
  five parse/re-check rounds and never deletes a route. Integration test
  on a fixture with a dead helper and a runtime class (`59ffc8a`).

### 1.4 Auto-PR generation

- `/plan "<story>"` creates branch `rivet/plan/<slug>`, writes the module
  and a SPEC.md, converges against parse + Gauntlet + a real `rivet
  build`, commits, and prints a PR body with the audit grade; `--push`
  opens the PR through `gh`. Code comes from an OpenAI-compatible
  provider (`RIVET_PLAN_BASE_URL`, `RIVET_PLAN_API_KEY`,
  `RIVET_PLAN_MODEL`) or a prepared module via `--from` — the
  deterministic path the gate exercises. Offline unit tests cover prompt
  assembly and response parsing; the integration test proves a fixture
  story lands on a branch whose crate compiles (`59ffc8a`).

### 1.5 Agentic diagnostics

- `suggested_fix` is now a required `String` on `Diagnostic` and
  `Finding`; every construction site carries a remediation written from
  its error code's meaning, and the JSON payload always emits the field
  (`52f87ed`). Parser error paths, every Gauntlet rule, and the store
  error paths assert non-empty fixes; a broad invariant test in
  `gauntlet/mod.rs` runs a fixture that trips E2043-E2046 and checks
  every output diagnostic (`52f87ed`).

## Repo maintenance

- `scripts/clean.sh` plus `make clean` / `make clean-all` remove the
  generated crates (`examples/*/generated`, about 140 MB each with their
  cargo targets) and the Rivet test fixtures in the temp dir; the old
  `make clean` pointed at a root `generated` path that does not exist
  (`53d70ca`).
- `rivet-cli/src/test_support.rs` adds a `ScratchDir` guard that removes
  each test fixture on drop, on success and on panic, and drops the
  run-pid from fixture names; before this the `/plan` fixture leaked a
  143 MB compiled crate per run (`4c638c7`).

## Phase 4 - Ecosystem and multi-service

Served by `prompts/prompt-07-ecosystem.md`. Entries land here as their
tracker lines finish; the section closes when the phase does.

### Plugin system

- Added the `rivet-plugin-api` workspace crate: the `Plugin` trait plus a
  generic `install` composition primitive, so every call site monomorphizes
  and the built binary carries a direct call instead of a registry lookup.
  `install` logs the plugin name once, at startup (`53242fd`).
- `rivet.toml` grows a `[plugins]` table (`crate`, `path`, `version`), and
  `rivet add plugin` records an entry while keeping every other line of the
  file byte for byte, comments included. A parent table the command creates
  is marked implicit, so a file gains no empty `[plugins]` header; the edit
  validates through the same resolver the build uses and writes nothing when
  it rejects (`E3016` for an unresolvable plugin, `E3017` for a file it
  cannot parse) (`53242fd`).
- `generate_project` resolves every plugin before it writes, adds one
  dependency per plugin to the generated manifest, and emits one install
  call per plugin into `main.rs` in name order — no `dyn`, no name table
  (`53242fd`).
- Shipped the reference plugin `examples/basic/plugins/auth-token` as a
  workspace member: it reads `RIVET_AUTH_TOKEN` once at install time, serves
  `GET /auth/check`, and compares the presented bearer token in constant
  time. The example project composes it (`53242fd`).

### Verification: plugin system

- 119 CLI tests plus 11 plugin tests and 1 doctest green; fmt, clippy
  (`-D warnings`), and `cargo deny check` clean; the gate passes end to end.
- End to end: `rivet build examples/basic/app.py` writes a manifest that
  depends on `rivet-plugin-auth-token` by path and a `main.rs` with one
  install call; the running binary answers `GET /ping` as before and
  `GET /auth/check` with `authenticated: false` without the header and
  `true` with the configured token (`53242fd`).

### Multi-service transport

- Split the generated crate into a transport-free service layer (one async
  function per route over plain Rust types) and an internal channel: a
  typed trait with `InProcess`, which monomorphizes to a direct call, and
  `Grpc`, which sends the same payload over a channel the app serves. The
  axum handler is now a thin wrapper over the channel and maps a channel
  failure to `502` (`a675668`).
- `[transport] mode = "in_process" | "grpc"` selects the topology at
  generation time, so the monolith carries no transport code, no `dyn`, and
  no gRPC dependency; `grpc_port` sets the channel port (`a675668`).
- Wrote the gRPC service by hand on `tonic::codec`, `tonic::server::Grpc`,
  and `tonic::client::Grpc`, with one JSON message per call: a generated app
  builds with only rustc and cargo, so the generated crate needs neither
  `protoc`, `tonic-build`, nor `prost`. Recorded in pillar 02, including the
  tonic 0.14 `router` feature note (`a675668`).
- `rivet trace` now reports the service function that holds the route logic
  and the channel-qualified registration line (`a675668`).

### Verification: multi-service transport

- 124 CLI tests green, including a two-topology integration test that builds
  one blueprint in `in_process` mode and again in `grpc` mode, runs both
  binaries, probes `GET /ping` and `POST /echo`, and asserts both modes
  answer the same body; fmt, clippy (`-D warnings`), `cargo deny check`, and
  the example build and audit all pass (`a675668`).
- The gRPC run binds its channel port while answering, so the HTTP responses
  in that run prove the channel round trip rather than a direct call
  (`a675668`).

### Polyglot dev proxy

- `rivet dev [app.py]` detects the frontend from its config file (Vite,
  Rsbuild, Next.js, Webpack) and serves one origin on the project's
  configured port: the blueprint's own routes and `/api/*` reach the
  generated backend, every other path reaches the frontend dev server, and
  a project with no frontend config sends every path to the backend
  (`44345fd`).
- The generated backend mounts the blueprint's paths and nothing else, so
  the proxy strips the `/api` prefix on the way upstream; a path the
  blueprint declares wins over the prefix and keeps its own path, so a
  route named `/api/orders` still resolves (`44345fd`).
- The proxy tunnels the frontend's HMR upgrade: it replays the request head
  on a raw connection, relays the `101`, and copies bytes in both
  directions with `copy_bidirectional`. A refused upgrade relays the
  frontend's own answer, and a frontend that never answers returns `502`
  with the reason. Handshake and upstream calls carry a 30-second deadline
  (`44345fd`).
- `rivet dev` binds the public port before it spawns the backend, so a bind
  failure cannot orphan the backend, and it stops the backend on exit or
  Ctrl-C. A frontend port that equals the proxy port is `E3018`
  (`44345fd`).
- Split the command into `dev/routing.rs` (the upstream decision and the
  prefix rewrite) and `dev/tunnel.rs` (the upgrade tunnel), each with a
  `tests.rs` sibling, so no file approaches the 400-line ceiling
  (`44345fd`).

### Verification: polyglot dev proxy

- 149 CLI tests green, including tunnel tests that drive the proxy with a
  raw socket: one relays a `101` and round-trips frames both ways, one
  asserts the rewritten path in the replayed request line, one relays a
  refused upgrade, and one reports `502` when the frontend port is dead;
  fmt, clippy (`-D warnings`), `cargo deny check`, and the example build
  and audit all pass through `./scripts/gate.sh` (`44345fd`).
- End to end on a Vite-shaped fixture with the example blueprint: `/ping`
  and `POST /echo` answer from the backend, `/api/ping` answers from the
  backend with the prefix stripped, `/` and `/assets/app.js` answer from
  the frontend, an upgrade on the proxy port returns `101` from the
  frontend and round-trips three frames, and the backend process is gone
  after `rivet dev` stops (`44345fd`).

### Embedded static assets

- `[frontend] dist` in `rivet.toml` names the production frontend build,
  and `rivet build` compiles that directory into the generated crate with
  `rust-embed`, so a shipped binary serves its frontend from its own
  memory with no sidecar files. The generator lives in
  `rivet-cli/src/transpiler/rust/assets.rs` and renders the `mod assets`
  block, the router's `.fallback(assets::serve)` line, and the
  compression layer (`7a3646f`).
- The single-page rule mirrors the frontend dev server's own rewrite: a
  `GET` or `HEAD` whose path names no file, from a client that accepts
  `text/html`, receives the embedded `index.html` when `spa = true`.
  Every other miss keeps its `404`, so an `XHR` to a backend path the
  blueprint does not serve does not receive HTML under `rivet dev`, whose
  backend runs the same generated binary (`7a3646f`).
- A path that percent-decodes outside the folder is refused, each asset
  carries a strong `ETag` from its SHA-256 hash with
  `Cache-Control: public, max-age=0, must-revalidate`, `If-None-Match`
  answers `304` with no body, and `HEAD` reports the file's length. A
  blueprint route always wins, because the assets mount as the fallback.
  A `[frontend] dist` that names a missing directory reports `E2004` as a
  warning, embeds nothing, and still compiles the crate; `Diagnostic` now
  has a `warning` constructor beside `blocker` (`7a3646f`).
- The generated router compresses every response with `tower-http`'s
  `CompressionLayer` and the `compression-br` feature, which supplies the
  Brotli support pillar 03 specified. Wire compression costs no binary
  size: the assets stay uncompressed inside the binary, and `rust-embed`'s
  own `compression` feature — Deflate and Zstd only, and a binary-size
  trade this phase does not measure — stays off (`7a3646f`).
- The example project gains a committed fixture build
  (`examples/basic/dist/`, the `.gitignore` `dist/` rule now excepts it)
  and the `[frontend]` section that embeds it (`7a3646f`).
- Recorded the choices in pillar 03 (`rust-embed` with `mime-guess` and
  `debug-embed`; `tower-http` Brotli) and aligned the phase-4 seed
  (`prompts/prompt-07-ecosystem.md`) with the shipped keys, which
  supersede its original `[assets] dir` shape (`7a3646f`).

### Verification: embedded static assets

- 198 workspace tests green, including an integration test that writes a
  fixture `dist/`, runs `rivet build`, renames the directory, starts the
  binary, and asserts: `/` and `/assets/main.js` return `200` with
  `text/html` and `text/javascript`, the body equals the fixture file, a
  blueprint route still answers, a browser navigation to `/orders/42`
  returns the index, a missing asset and a non-HTML client both keep
  their `404`, `/../app.py` never returns `200`, `HEAD` reports the
  length, `If-None-Match` answers `304`, and `Accept-Encoding: br`
  returns a `content-encoding: br` body shorter than the file; fmt,
  clippy (`-D warnings`), `cargo deny check`, and the example build and
  audit all pass through `./scripts/gate.sh` (`7a3646f`).
- End to end on `examples/basic` with `dist/` renamed to `dist-renamed`:
  `GET /ping` answers `{"status":"pong"}`, `GET /` and `GET /orders/42`
  answer the embedded `index.html`, `GET /assets/main.js` answers with
  `text/javascript`, `GET /assets/gone.js` and `GET /api/orders` with
  `Accept: */*` answer `404`, `POST /ping` answers `405` with
  `Allow: GET, HEAD`, and the index drops from 494 bytes to 276 bytes on
  the wire under `Accept-Encoding: br` while decoding byte-for-byte
  identical to the file (`7a3646f`).

### Admin panel

- `[admin] enabled = true` renders the blueprint's route table at build
  time and mounts two read-only endpoints: `GET /__rivet/routes` answers
  the table as JSON (`method`, `path`, `handler`, and the route's story
  IDs), and `GET /__rivet/` answers a one-file HTML panel that renders it.
  The table is a static string, so the request path does no serialization
  and holds no state, and the module is emitted only when the opt-in is set
  (`0851b0d`).
- The panel is one embedded HTML file with no dependencies. Rivet has no
  Node toolchain in its build, so there is no React or Solid step; the
  decision is recorded in pillar 03 rather than promised in a config key
  (`0851b0d`).
- `rivet build` rejects a blueprint route on either panel path with `E2006`
  and a fix, instead of generating a router that panics on an overlapping
  route at startup (`0851b0d`).
- The example project enables the panel, so the gate builds and audits it
  (`0851b0d`).

### Verification: admin panel

- End to end on `examples/basic`: `GET /__rivet/routes` answers
  `[{"handler":"ping","method":"GET","path":"/ping","stories":["US-001"]},
  {"handler":"echo","method":"POST","path":"/echo","stories":["US-002"]}]`
  with `content-type: application/json`, `GET /__rivet/` answers the 1581
  byte panel with `text/html; charset=utf-8`, and `GET /ping` and
  `GET /auth/check` still answer from the compiled app and plugin
  (`0851b0d`).
- An integration test builds a panel-enabled fixture, starts the binary,
  asserts the route table names both routes with their stories, asserts the
  panel answers `text/html` and contains its own fetch path, and asserts
  the app's own routes still answer (`0851b0d`).

### Service discovery

- `[discovery] backend = "consul" | "etcd"` registers the generated app at
  startup and removes it on shutdown. Consul posts the service `ID`, `Name`,
  and `Port` to `/v1/agent/service/register` and removes it through
  `/v1/agent/service/deregister/<name>`; etcd puts a base64 key at
  `/rivet/services/<name>` on a 60-second lease and re-grants it every 20
  seconds, so a process that dies without deregistering expires on its own
  (`857060d`).
- The client is a hand-written HTTP/1.1 client in the generated crate, one
  connection per request, plain `http://` only: registering happens once, so
  an HTTP stack would cost more than it saves, and a generated app must
  build with rustc and cargo alone. Every request carries a five-second
  deadline, connect included (`857060d`).
- Registration never stops the app: a registry that refuses, errs, or does
  not answer within the deadline warns, and the app serves. `rivet build`
  rejects only a service name that cannot go into a registry path or key,
  with `E2005` and a fix (`857060d`).
- Shutdown is graceful: `main` serves the app with
  `with_graceful_shutdown`, and the generated manifest gains the `signal`,
  `time`, and `io-util` tokio features only when discovery is configured
  (`857060d`).

### Verification: service discovery

- An integration test runs the generated binary against a stub registry and
  asserts the wire format the client sends: the request line is
  `PUT /v1/agent/service/register HTTP/1.1`, `host` names the stub, the
  `content-type` is `application/json`, the `content-length` matches the
  body, and the body reads `"Name":"orders-api"` and `"Port":4333`. It then
  sends SIGINT and asserts a second request to
  `PUT /v1/agent/service/deregister/orders-api`, proving the graceful
  shutdown path leaves the registry (`857060d`).
- A second integration test runs a registry that accepts the connection and
  never answers: the run waits the five-second deadline, warns, and the app
  still answers `GET /ping` (`857060d`).
- The unit tests assert the generated wiring for both backends, the etcd
  base64 key against the RFC 4648 vectors, and the `E2005` rejection of a
  service name that cannot go into a registry path (`857060d`).

### Story-to-tracker sync

- `rivet sync [app.py] [--dry-run] [--apply] [--from FILE]` reconciles the
  blueprint's story IDs with Jira or Linear and reports four differences:
  a story with no issue, an issue with no story, a title that no longer
  matches the routes the story covers, and an issue the tracker closed while
  the blueprint still serves it. Nothing is written without `--apply`, and
  `--apply` only creates the missing issues (`7605413`, `c5e79d5`).
- An issue binds to a story through its title — `<id>: <METHOD> <path>[; ...]`
  — and the binding is exact: tracking compares the title's prefix before
  the first colon to the story ID, so every ID the decorator accepts
  round-trips. Only the orphan check uses a shape heuristic, so an ordinary
  issue such as `Fix the flaky test: again` contributes no noise. A story ID
  the title cannot carry, such as one holding a colon, fails with `E3023`
  (`c5e79d5`).
- Both readers page to the end of the tracker: Jira on `nextPageToken` from
  `GET /rest/api/3/search/jql` (the classic `/search` is marked "Currently
  being removed" in the REST v3 reference), Linear on `pageInfo` cursors. A
  tracker whose paging never converges fails with `E3020` after a bounded
  number of pages, so `--apply` cannot file duplicates on every run
  (`c5e79d5`).
- Linear's `issueCreate` takes a team ID while the configuration names a
  team key, so the creation path resolves one into the other with a `teams`
  query first. Jira's creation posts to `/rest/api/3/issue` (`c5e79d5`).
- Configuration comes from the environment only, never from the repository:
  `RIVET_JIRA_BASE_URL`/`RIVET_JIRA_TOKEN`/`RIVET_JIRA_PROJECT` or
  `RIVET_LINEAR_TOKEN`/`RIVET_LINEAR_TEAM`. Neither, or both, is `E3019`; a
  failed read is `E3020`, a failed write is `E3021`, and a diff that
  survives the run is `E3022` (`7605413`, `c5e79d5`).
- `--from FILE` reads a captured payload instead of calling the tracker, and
  with no credentials it reads the payload's own shape to pick the parser,
  so the offline path needs no secrets (`c5e79d5`).
- Pillar 05 documents the check the Gauntlet makes (`E2045`) and the
  reconciliation `rivet sync` adds, including the binding rule, the wire
  format, and paging (`7605413`, `c5e79d5`).

### Verification: story-to-tracker sync

- 47 sync tests green, covering the diff computation (missing, orphan, title
  drift, state drift, both drifts on one issue, and an unrelated issue
  ignored), both providers' request builders and response parsers against
  captured payloads, the command end to end against a fixture app, and the
  `E3023` rejection (`7605413`, `c5e79d5`).
- A stub tracker is the oracle for the client: the paging test asserts both
  pages are requested and that the second request carries the token the
  first answer named; a repeated-token test asserts `E3020`; a
  token-alternating test asserts the page cap bounds the loop; and the
  `--apply` test asserts one read plus one `POST /rest/api/3/issue`, an
  agreeing outcome, and exit 0 (`c5e79d5`).
- Offline smoke test with no credentials:
  `rivet sync examples/basic/app.py --dry-run --from captured.json` reports
  `0 missing, 0 orphan, 1 drifted` and exits 1 with `E3022`; the same app
  against an agreeing payload prints `diff: the blueprint and the tracker
  agree` and exits 0; an app whose story holds a colon exits 1 with `E3023`
  pointing at the module path (`c5e79d5`).

### Phase-4 docs and tracker close

- Pillars 01-03 describe what shipped: plugin composition (01), the
  transport switch with service discovery (02), and `rivet dev`, the
  embedded assets, and the admin panel (03). Pillar 05 gained the sync
  design (`0851b0d`, `857060d`, `7605413`, `c5e79d5`).
- `docs/ROADMAP.md` phase 4 is closed: every deliverable box is ticked, with
  the success metric and each feature's evidence recorded under it
  (`0851b0d`, `857060d`, `7605413`, `c5e79d5`).
- The README status paragraph names the plugin system, the transport switch,
  `rivet dev`, the embedded assets, the admin panel, service discovery, and
  `rivet sync`.
- Fixed a repository defect the close surfaced: `.gitignore` held a bare
  `build/`, which also matched `rivet-cli/src/commands/build/`, so every
  integration test in that directory — transport, embedded assets, the admin
  panel, discovery — was untracked and a fresh clone ran the gate without
  them. `dist/` still covers a frontend's production output (`4263fc3`).
- Every finished phase-4 line moved to this file with its commit hash.

## Phase 5 - WASM and mobile

Served by `prompts/prompt-08-wasm-mobile.md`, authored before the phase
started the way every phase before it was. Entries land here as their tracker
lines finish; the mobile lines stay open until their platform toolchains
exist.

### The phase-5 seed

- `prompts/prompt-08-wasm-mobile.md` names the four deliverables, the
  verification each one needs, and the repository rules that hold: the
  WASM target reuses the native service layer rather than copying it, a
  platform this repository cannot build is recorded as blocked with the
  toolchain it needs, and generator output no toolchain here has compiled
  does not ship (`ec45c3d`).

### The WASM target

- `rivet build --target wasm` renders the blueprint as a WASI command module
  and compiles it for `wasm32-wasip1`. The crate carries the same DTO structs
  and the same `mod service` as the native target, so a route's business
  logic has one implementation; what it drops is everything that needs a
  server. The manifest depends on serde and serde_json alone — no axum, no
  tokio, no gRPC, no plugins, and no embedded assets (`ddd4d45`).
- The module is an edge handler, not a server: it reads one request as JSON
  on stdin (`{"method": "GET", "path": "/ping"}`), dispatches it through a
  `match` rendered at build time, and writes one envelope to stdout
  (`{"status": 200, "body": {...}}`). A path no route declares answers `404`;
  a declared path with a method it does not serve answers `405` and names the
  methods it allows; a body that does not match the declared type, or an
  unreadable request, answers `400` (`ddd4d45`).
- The executor is one poll with `Waker::noop()`. Every generated route is an
  `async fn` that never awaits — the parser's subset is literals, request
  parameters, and one DTO construction — so the module carries no runtime and
  no reactor (`ddd4d45`).
- `rivet build` gained `--target native|wasm` and defaults to `native`, so
  every existing invocation behaves as before. A missing `wasm32-wasip1`
  target is `E2010` with the `rustup target add` command that fixes it, not a
  wall of cargo output (`ddd4d45`).
- `docs/pillars/08-wasm-mobile-sdk-support.md` documents the target matrix,
  the request protocol, why the module has no reactor, what the target
  deliberately omits (WASI sockets, `[frontend] dist`, plugins, and
  discovery), and which toolchain each mobile deliverable still needs.
  `docs/ROADMAP.md` ticks the WASM box and leaves the two mobile boxes open
  (`ddd4d45`).

### Verification: the WASM target

- Built `examples/basic/app.py` for the target and ran the module on its real
  platform: `wasmtime run` with `{"method":"GET","path":"/ping"}` answers
  `{"body":{"status":"pong"},"status":200}`; `POST /echo` with
  `{"hello":"world"}` answers `{"body":{"echo":{"hello":"world"}},"status":200}`;
  `GET /nope` answers `404`; `DELETE /ping` answers
  `{"body":{"error":"DELETE is not served by /ping; it allows GET"},"status":405}`;
  and a non-JSON request answers `400` (`ddd4d45`).
- Two integration tests drive the same protocol. One builds the crate for the
  host and runs it, which proves the dispatch, the four statuses, and the
  envelope without a WASI host; the other builds the module and runs it under
  Wasmtime, and reports the missing host instead of passing silently when
  `wasmtime` is absent (`ddd4d45`).
- Five renderer tests assert the wasm manifest carries no native dependency,
  the crate reuses `mod service` and the DTO structs, the dispatch names
  every route, a body route deserializes its declared type, and every
  template token is substituted (`ddd4d45`).

### The const_generics flag

- `[rust_native_features] const_generics = true` renders a `List[T, N]` DTO
  field as a fixed-size Rust array instead of failing with `E2003`, and the
  example config sets the flag. Without the opt-in the generator reports the
  same blocker it always did, with a fix that names the flag.
- serde derives `Serialize` and `Deserialize` for arrays up to 32 elements,
  so a larger `[T; N]` borrows a generated bridge
  (`#[serde(with = "fixed_array")]`): serializing goes through a slice, and
  deserializing builds the array from a `Vec` and rejects a length that does
  not match the declaration. The bridge lives beside the two helpers the
  service layer emits, so it travels into both targets.
- The example app gained an `Embedding` DTO with `List[float, 768]` and a
  `POST /embed` route, so the gate exercises the feature end to end.
- Three more generator gaps closed with the flag: an array literal now
  renders `[...]` for a fixed-size target and fails with `E2011` when its
  element count does not match the declaration (it used to emit `vec![...]`
  against a `[T; N]` field, which cargo reported as a generator fault);
  `Optional[List[T, N]]` is rejected with `E2012` rather than generating a
  struct that cannot compile; and a flag the generator does not implement is
  rejected with `E2013`, so the section cannot advertise a capability the
  build ignores (`927fb68`).

### Verification: the const_generics flag

- End to end on the native target: `POST /embed` with 768 values answers all
  768 back in order, and a 10-value request answers `422` from the
  deserializer.
- End to end on the WASI target: `wasmtime run` answers `200` for a
  768-element embedding and `400` for a short one (`927fb68`).
- The wasm integration fixture carries a fixed-size array DTO, and both
  protocol runs assert the array round-trips and that a short one is
  rejected, so a bridge emitted for one target and not the other would fail
  the gate instead of shipping.
- Generator tests cover the render, the missing opt-in (`E2003`), the absent
  bridge for a plain array, the short literal (`E2011`), the optional form
  (`E2012`), and the unimplemented flag (`E2013`); the config tests cover
  the defaults and the flag the generator does not implement (`927fb68`).

### Blueprint checks the generator makes

- The generator rejects two routes that serve the same method and path with
  `E2014`, naming the method, the path, and both handlers. The Gauntlet's
  duplicate rule compares handler bodies, so routes that share a route but
  differ in body passed it; the native router then panicked at startup after
  the build reported success, and the WASI dispatch would have kept only the
  first arm, so the two targets disagreed on the same blueprint.
- Both `generate_project` and `generate_wasm_project` call the check, so one
  blueprint gets one answer whichever target is built. The diagnostic
  carries no file location, because the generator holds no app path. Pillar
  03 documents both build-time route checks beside the admin panel
  (`565f4c2`).

### Verification: the blueprint checks

- Reproduced both halves before the fix: two `GET /ping` routes with
  different bodies built successfully, emitted two `.route("/ping", …)`
  calls, and the binary panicked with "Overlapping method route".
- After the fix, native and wasm builds both answer `E2014` with
  ``two routes serve `GET /ping`: `ping` and `ping_again` ``, and
  `examples/basic` still builds on both targets (`565f4c2`).
- Three generator tests cover the rejection, the wasm target's agreement,
  and the negative case: the same path on two methods is two valid routes
  (`565f4c2`).
- A second overlap shape is rejected at the parser: two routes that share a
  handler name. The generated `mod service`, the channel trait, and the
  handlers each defined the name twice, and cargo reported "defined multiple
  times" against generated code. The check sits in parser pass two beside
  the `E1003` duplicate-DTO guard, so it carries the real file and line and
  every parsing command rejects the module — build, audit, trace, plan, and
  mcp — not only the two generators. The message names the first
  definition's line (`31c3a7e`).
