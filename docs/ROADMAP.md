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
- [ ] Complexity Walker (CC < 8) - fails build if exceeded.
- [ ] Type Checker (0 `any`/`unknown`) - rejects dynamic types.
- [ ] Duplicate Code Detector (AST hashing) - blocks redundant code.
- [ ] Mutation Tester (`cargo-mutants` integration) - requires 100% survival.

**Success Metric**: A PR with "bad" code (CC > 8) fails the build with a machine-readable JSON error.

---

## Phase 2: The Context Engine (Weeks 5-6)

**Goal**: Persist state for humans and AI agents.

**Deliverables**:
- [ ] SQLite schema: commands, sessions, AST fingerprints.
- [ ] LanceDB vector store for semantic code search.
- [ ] `rivet session save` - dumps current state.
- [ ] `rivet session resume` - restores state with compacted Markdown dump.
- [ ] `rivet explain "bug"` - traces the commit that introduced a bug.

**Success Metric**: `rivet explain "TypeError on line 42"` returns a human-readable summary with the offending commit.

---

## Phase 3: MCP & Agentic CLI (Weeks 7-8)

**Goal**: First-class AI integration.

**Deliverables**:
- [ ] MCP server exposing AST and Vector DB.
- [ ] Slash-commands: `/plan`, `/fix`, `/trace`.
- [ ] Auto-PR generation with performance benchmarks.
- [ ] Agentic error handling (JSON errors with suggested fixes).

**Success Metric**: `rivet /plan "Add referral codes"` generates a working PR with passing tests.

---

## Phase 4: Ecosystem & Multi-Service (Weeks 9-10)

**Goal**: Production readiness.

**Deliverables**:
- [ ] Plugin system (compile-time composition via traits).
- [ ] Service discovery (Consul/etcd/Nacos).
- [ ] Built-in Admin Panel (React/Solid).
- [ ] Story-to-Jira/Linear sync (`rivet sync`).
- [ ] Multi-service architecture (monolith → microservices via config).

**Success Metric**: `rivet add plugin auth-oauth2` compiles the plugin in without runtime overhead.

---

## Phase 5: WASM & Mobile (Weeks 11-12)

**Goal**: Edge and native distribution.

**Deliverables**:
- [ ] WASM compile target (`wasm32-wasi`).
- [ ] UniFFI bindings for Kotlin (Android), Swift (iOS), and TypeScript (React Native).
- [ ] Mobile SDK generation (`rivet mobile init`).

**Success Metric**: `rivet mobile init --platforms ios,android` generates SDKs that compile and pass tests.

---

## Post-v1 (Future)

- Real-time WebSocket support (Plugin).
- GraphQL federation.
- Distributed tracing UI.
- Edge deployment (Cloudflare Workers, Fly.io).
- More language parsers (Java, Go, C#).