# Rivet v1 Roadmap

**Mission**: Deliver a stable, open-source framework that forces high-quality code and enables AI-assisted development.

**Timeline**: 12 weeks for v1 (3 months).

---

## Phase 0: The Spike (Weeks 1-2)

**Goal**: Prove the transpiler works.

**Status**: Complete. `rivet build examples/basic/app.py` parses the Python
DSL, generates an axum crate, compiles it, and the binary answers `/ping` and
`/echo`. See `examples/basic/`.

**Deliverables**:
- [x] CLI parses a Python DSL function with `@api.post`.
- [x] CLI generates a valid `axum` Rust server.
- [x] `curl localhost:3000/ping` returns `{"status":"pong"}`.
- [ ] Docker/Orbstack environment runs the generated binary.

**Success Metric**: A developer can write 5 lines of Python and get a compiled
Rust binary. Met: the basic example is 12 lines of Python and builds to a
standalone binary.

**Risks**:
- `tree-sitter-python` struggles with type hints → Use fallback parser.
- Axum requires `Send + Sync` → Wrap handlers in `Arc`.

---

## Phase 1: The Gauntlet (Weeks 3-4)

**Goal**: Enforce strict quality at compile time.
**Deliverables**:
- [x] Complexity Walker (CC < 8) - fails build if exceeded.
- [x] Type Checker (0 accidental any/unknown) - rejects dynamic types
  outside the documented JSON boundary.
- [x] Duplicate Code Detector (AST hashing) - blocks redundant code.
- [x] Story gate and dead-code rules (pillar 05) - every route tagged.
- [x] `rivet audit` reports the MQI grade and a JSON breakdown (pillar 06).
- [x] CI runs the Gauntlet on `examples/basic` and scans dependencies with
  cargo-deny.
- [ ] Mutation Tester (`cargo-mutants` integration) - requires 100%
  survival; stays open, blocked on the generated-code test story (see
  pillar 06, `not_scored`).

**Status**: Closed except the mutation tester. `rivet build` enforces the
five rules (`E2042`-`E2046`) between parse and generate; `rivet audit`
grades the module on the phase-1 MQI dimensions; CI gates the example and
scans the dependency tree. See `tasks/completed.md`.

---

## Phase 2: The Context Engine (Weeks 5-6)

**Goal**: Persist state for humans and AI agents.

**Deliverables**:
- [x] SQLite schema: commands, sessions, AST fingerprints.
- [x] LanceDB vector store for semantic code search.
- [x] `rivet session save` - dumps current state.
- [x] `rivet session resume` - restores state with compacted Markdown dump.
- [x] `rivet explain "bug"` - traces the commit that introduced a bug.

**Success Metric**: `rivet explain "TypeError on line 42"` returns a human-readable summary with the offending commit.

---

## Phase 3: MCP & Agentic CLI (Weeks 7-8)

**Goal**: First-class AI integration.

**Deliverables**:
- [x] MCP server exposing AST and Vector DB.
- [x] Slash-commands: `/plan`, `/fix`, `/trace`.
- [x] Auto-PR generation with performance benchmarks.
- [x] Agentic error handling (JSON errors with suggested fixes).

**Status**: Complete. `rivet mcp` serves the parser, audit, vector, and
context-store tools over stdio on the official rmcp SDK; `/plan` turns a
story into a verified `rivet/plan/*` branch with a spec and audit grade
(BYO LLM provider or `--from`), `/fix` applies the Gauntlet's
deterministic repairs, `/trace` follows a request to its introducing
commit, and every JSON diagnostic carries a `suggested_fix`. See
`tasks/completed.md` and `docs/pillars/09-super-cli.md`.

---

## Phase 4: Ecosystem & Multi-Service (Weeks 9-10)

**Goal**: Production readiness.

**Deliverables**:
- [x] Plugin system (compile-time composition via traits).
- [x] Polyglot dev proxy (`rivet dev`) for Vite, Rsbuild, Next.js, and
  Webpack, HMR tunnel included.
- [x] Static assets embedded in the binary (`rust-embed`).
- [x] Service discovery (Consul and etcd).
- [x] Built-in admin panel: one embedded HTML file, no build step.
- [ ] Story-to-Jira/Linear sync (`rivet sync`).
- [x] Multi-service architecture (monolith → microservices via config).

**Success Metric**: `rivet add plugin auth-token --path
plugins/auth-token` composes the plugin into the generated binary with one
monomorphized install call and no runtime lookup. Met: the example project
composes `auth-token`, the generated manifest depends on the plugin crate,
and `GET /auth/check` answers from the compiled plugin.

The static assets are met: `[frontend] dist` compiles the production build
into the generated crate with `rust-embed`, the router mounts the embedded
directory as its fallback, and an integration test builds the fixture,
renames `dist/`, and proves the binary still serves `index.html` and its
assets. See `docs/pillars/03-polyglot-frontend-support.md`.

The dev proxy is met: `rivet dev` detects the frontend from its config
file, serves the blueprint's routes and `/api/*` from the Rust backend with
the prefix stripped, sends every other path to the frontend dev server, and
tunnels the frontend's HMR upgrade. See
`docs/pillars/03-polyglot-frontend-support.md`.

The admin panel is met: `[admin] enabled = true` compiles the blueprint's
route table into the generated binary, `GET /__rivet/routes` answers it as
JSON, and `GET /__rivet/` answers a single-file panel that renders it. See
`docs/pillars/03-polyglot-frontend-support.md`.

Service discovery is met: `[discovery]` registers the generated app with
the Consul agent or etcd at startup, deregisters it on a graceful shutdown,
and never lets a registry outage stop the app. An integration test runs the
registration against a stub registry and asserts the payload names the
service and its port, then sends SIGINT and asserts the deregistration.
See `docs/pillars/02-multi-service-architecture.md`.

---

## Phase 5: WASM & Mobile (Weeks 11-12)

**Goal**: Edge and native distribution.

**Deliverables**:
- [ ] WASM compile target (`wasm32-wasi`).
- [ ] UniFFI bindings for Kotlin (Android), Swift (iOS), and TypeScript (React Native).
- [ ] Mobile SDK generation (`rivet mobile init`).

**Success Metric**: `rivet mobile init --platforms ios,android` generates SDKs that compile and pass tests.

**Scope note**: this project finishes the Python front end before it starts
any TypeScript work. The TypeScript (React Native) binding stays deferred
until the Python-side phases are done; the Kotlin and Swift bindings do not
wait. See the scope section in `tasks/todo.md`.

---

## Post-v1 (Future)

- Real-time WebSocket support (Plugin).
- GraphQL federation.
- Distributed tracing UI.
- Edge deployment (Cloudflare Workers, Fly.io).
- More language parsers (Java, Go, C#), and a TypeScript DSL front end over
  the same IR once the Python front end is complete.