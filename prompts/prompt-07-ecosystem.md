# Prompt 07: Ecosystem and Multi-Service

**Objective**: Make a Rivet app a production system, not a single binary.
Ship four things that turn the transpiled server into a deployable service:
a compile-time plugin system, one internal-channel abstraction that runs
in-process or over gRPC by configuration, a polyglot `rivet dev` proxy, and
embedded static assets. Then close the loop outward: service discovery, an
admin panel that lists the routes the app actually serves, and `rivet sync`
that reconciles blueprint story IDs with Jira or Linear.

**Context**: Phases 0-3 built the pipeline (`parse` → Gauntlet → generate →
`cargo build`), the `.rivet/` context engine, and the MCP and slash-command
surfaces. Everything so far assumes one blueprint, one binary, one process.
Phase 4 is the first phase whose subject is the *runtime topology* of the
generated app, so it is the first phase that changes what `generate_project`
emits as a system rather than as a router.

Three pillars define the phase:

- `docs/pillars/01-compile-time-ioc.md` — dependencies resolve at compile
  time, with no registry and no reflection.
- `docs/pillars/02-multi-service-architecture.md` — services talk over
  strongly typed internal channels, so the same code runs as a monolith
  (in-memory) or as microservices (remote) by changing one config flag.
- `docs/pillars/03-polyglot-frontend-support.md` — `rivet dev` proxies
  `/api/*` to the Rust backend and everything else to the frontend dev
  server; production embeds `dist/` in the binary.

All three pillars are stubs today. Finish each one in the docs commit that
lands its feature.

Two design rules carry over from phase 3 and hold here:

- **Token economy is not at stake in this phase, but runtime economy is.**
  A plugin, a channel, and an embedded asset must all be zero-cost in the
  request path: no `dyn` dispatch, no registry lookup, no per-request map.
- **The provider is the only external system that speaks a wire format.**
  Everything the generated crate needs is in the workspace tree or a pinned
  crate. Do not add a build-time dependency the user's machine may not have.

**Non-negotiables**: `rivet-core` stays the IR contract and gains no HTTP,
database, or network dependency. New error codes continue the context-engine
range after `E3015`. No `unwrap`/`expect` outside tests. No stubs: a shipped
plugin, channel, or panel does its documented job or it is not shipped.

---

## Tasks

### 1. Compile-time plugin system, composed via traits

A Rivet plugin is a Rust crate that adds routes or middleware to a generated
app, resolved at generation time. There is no registry and no runtime
lookup: `rivet build` emits one monomorphized call per configured plugin.

Add a small workspace crate `rivet-plugin-api` that owns the contract:

```rust
pub trait Plugin: Send + Sync + 'static {
    fn name(&self) -> &'static str;
    fn install(self, router: axum::Router) -> axum::Router;
}

pub fn install<P: Plugin>(router: axum::Router, plugin: P) -> axum::Router;
```

`install` is generic, so the generated call site monomorphizes: the compiler
resolves the plugin at build time and inlines it. A plugin crate exports a
unit type `Plugin` that implements the trait; that is the whole convention.

Declare plugins in `rivet.toml`:

```toml
[plugins.auth-token]
crate = "rivet-plugin-auth-token"  # optional; defaults to rivet-plugin-<name>
path = "plugins/auth-token"        # optional; relative to the project dir
version = "0.1"                    # used when `path` is absent
```

Add `rivet add plugin <name> [--path <dir>] [--crate <crate>]
[--version <v>] [--app <app.py>]`. The command is idempotent: adding a
plugin that is already configured updates that entry and leaves the rest of
the file alone. Record the invocation in the store like every other command.

`generate_project` then:

- adds one dependency per plugin to the generated `Cargo.toml` (`path` for
  a local plugin, `crate = version` otherwise);
- emits `let app = rivet_plugin_api::install(app, <crate>::Plugin);` once
  per plugin, in file order, before the server starts;
- keeps the generated `main.rs` free of `dyn`, `Box<dyn Plugin>`, and any
  plugin-name string comparison.

Ship one real fixture plugin, `examples/basic/plugins/auth-token`, as a
workspace member. It reads `RIVET_AUTH_TOKEN` at install time, contributes
`GET /auth/check` returning `{"authenticated": <bool>, "scheme": "bearer"}`,
and compares the request's bearer token against the configured one with a
constant-time comparison. It must not weaken or gate any existing route.

- Acceptance: `rivet build` on the example writes a crate whose manifest
  depends on the plugin and whose `main.rs` calls `rivet_plugin_api::install`
  once, with no `dyn` and no registry; the built binary answers
  `GET /auth/check` with `authenticated: false` when no header is sent and
  `true` when the configured token is sent.

### 2. Multi-service switch: in-process or gRPC by config

The generated app must run the same handler logic over two topologies. Give
the generator a transport-free service layer and one channel abstraction:

- `service::*` — one async function per route, holding the handler logic and
  speaking JSON values only (no axum types).
- `channel::Channel` — a trait with one async `call(method, payload)`
  operation, plus `InProcess`, which calls `service::*` directly, and
  `Grpc`, which sends the same payload over a real gRPC channel.

`rivet.toml` selects the topology:

```toml
[transport]
mode = "in_process"   # or "grpc"
grpc_port = 50051     # used when mode = "grpc"
```

In `in_process` mode the axum handlers call `service::*` through
`InProcess`, which monomorphizes to a direct call — the monolith, with no
serialization in the path. In `grpc` mode the same handlers call through
`Grpc`, and the binary also serves that channel on `grpc_port`, so a second
process running the same blueprint can act as the remote service.

The gRPC service carries JSON payloads over a hand-written `tonic::codec`
codec. Do not add `tonic-build`, `prost`, or a `protoc` requirement: the
generated crate must build on a machine with only rustc and cargo.

- Acceptance: the example builds and runs in both modes; `curl /ping`
  returns the same body in both; a test drives `Channel::call` through both
  implementations against the same fixture service and gets equal results.

### 3. `rivet dev`: polyglot frontend proxy

Detect the frontend in the project directory from its config file:
`vite.config.*`, `rsbuild.config.*`, `next.config.*`, `webpack.config.*`.
Report the detected framework, or `none` when nothing is found.

Then start the development topology:

- proxy on the project's configured host and port;
- `/api/*` (the route prefix the blueprint uses) forwards to the Rust
  backend;
- every other path forwards to the frontend dev server;
- framework default dev ports, overridable with `--frontend-port`, plus
  `--backend-port` and `--app`.

Forward the method, path, query, headers, and body, and return the upstream
status, headers, and body unchanged. A request that no upstream answers
returns 502 with a diagnostic, never a hang.

- Acceptance: with a fixture Vite-shaped project and a stub frontend
  server, `curl /api/ping` proxies to the Rust backend and `curl /` proxies
  to the frontend; detection names `vite` for the fixture and `none` for a
  directory with no frontend config.

### 4. Static assets embedded in the binary

`[assets] dir = "dist"` in `rivet.toml` embeds a built frontend into the
generated binary with `rust-embed`. The generated app serves those files at
every path that is not a route, so production needs no filesystem beside
the binary. Content type comes from the file extension; a missing file
falls through to 404, and an absent `dir` leaves the app route-only with no
generated asset code.

- Acceptance: a built binary serves a fixture `dist/index.html` after the
  `dist/` directory is renamed; the served body and content type match.

### 5. Service discovery and the admin panel

**Discovery.** `[discovery] backend = "consul" | "etcd"`, plus `url`,
`service_name`, and `service_port`. On startup the generated app registers
itself through the backend's HTTP API (Consul agent service registration,
etcd lease plus key); on shutdown it deregisters. Registration failure
prints a warning and does not stop the server.

**Admin panel.** `[admin] enabled = true` serves two endpoints from the
generated app: `GET /__rivet/routes` returns the route table as JSON
(method, path, handler, stories), and `GET /__rivet/` returns an embedded
single-file HTML panel that renders that table. The panel is dependency-free
HTML and JavaScript compiled into the binary. Rivet has no Node toolchain in
its build, so the panel ships as one static file: record that decision in
pillar 03 rather than promising a React or Solid build step.

- Acceptance: an integration test runs the registration against a stub
  registry and asserts the payload names the service and its port; the
  running example answers `/__rivet/routes` with its route table and
  `/__rivet/` with the panel.

### 6. `rivet sync`: story-to-tracker reconciliation

`rivet sync --dry-run` reads the story IDs from the blueprint, lists the
issues in the configured tracker, and prints the diff: stories with no
issue, issues with no story, and issues whose state or title drifted. The
command never writes unless `--apply` is passed; `--apply` creates the
missing issues.

Two providers, both HTTP JSON and both configured from the environment,
never from the repo:

- `RIVET_JIRA_BASE_URL`, `RIVET_JIRA_TOKEN`, `RIVET_JIRA_PROJECT`
- `RIVET_LINEAR_TOKEN`, `RIVET_LINEAR_TEAM`

Keep each provider a small module that builds one request and parses one
response, so request assembly and response parsing are unit-testable
offline from captured payloads. The network call is not exercised by the
gate; the diff computation is.

- Acceptance: `rivet sync --dry-run` against a fixture app and a captured
  tracker payload reports the expected diff; a fixture app with no missing
  issues reports an empty diff and exits 0.

### 7. Docs and tracker

In the commit that lands each feature:

- Finish the pillar doc that the feature serves: pillar 01 for the plugin
  system, pillar 02 for the transport switch, pillar 03 for `rivet dev`,
  embedded assets, and the panel.
- Tick the matching `docs/ROADMAP.md` phase-4 checkbox.
- Update the README status paragraph and repository layout when a new crate
  or directory appears.
- Move each finished `tasks/todo.md` line to `tasks/completed.md` with the
  finishing commit hash, in a follow-up commit.

Acceptance Criteria
- A plugin configured in `rivet.toml` compiles into the generated app with
  one monomorphized `install` call, no `dyn`, and no registry lookup, and
  its route answers a request.
- One blueprint builds and runs in both `in_process` and `grpc` transport
  modes, and a test exercises both channel implementations.
- `rivet dev` proxies `/api/*` to the backend and other paths to the
  frontend, with the framework detected from its config file.
- A built binary serves an embedded asset with no `dist/` directory on disk.
- Discovery registration posts the service and port to the registry API, and
  `/__rivet/routes` lists the route table the app serves.
- `rivet sync --dry-run` reports the story diff against a captured tracker
  payload and writes nothing.
- `cargo fmt --all -- --check`, `cargo clippy -- -D warnings`, and
  `cargo test --workspace` all pass; `./scripts/gate.sh` is green.

Agent Instructions
1. Follow the existing module layout: one module per command under
   `rivet-cli/src/commands/`, small modules, `tests.rs` siblings for tests.
2. Every generator change is verified end to end: rebuild `examples/basic`,
   run the binary, and curl the affected routes. A unit test on generated
   text is not proof that the generated crate compiles.
3. Keep generated code zero-cost: concrete types, monomorphized calls, no
   runtime registry, no per-request allocation that a direct call avoids.
4. Configuration grows `rivet.toml` in sections. Unknown sections stay
   ignored, and a missing section keeps today's behavior, so the example
   keeps working without a config edit.
5. New error codes continue the context-engine range after `E3015`. State
   the code's meaning in its `suggested_fix`.
6. Build JSON by hand from `serde_json::Value`. Never use the `json!` macro,
   `unwrap`, or `expect` outside tests.
7. Add a workspace crate only when it earns its place, and register it in
   `check-file-length`'s scanned trees and the workspace members list.
8. Never import network, database, or HTTP types into `rivet-core`.
9. `make clean` after the session regenerates crates; tests clean their own
   fixtures through the `ScratchDir` guard.

Output
A Rivet app that deploys: plugins composed at compile time with no runtime
lookup, one route body that runs in-process or over a network by config,
`rivet dev` behind a frontend dev server, assets inside the binary, a
service that registers itself and shows its routes, and a story tracker
that stays in sync — all recorded in pillars 01, 02, and 03, the roadmap,
and the tracker.
