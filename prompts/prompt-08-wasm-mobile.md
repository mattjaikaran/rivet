# Prompt 08: WASM and Mobile

**Objective**: Ship the same blueprint to an edge runtime and to native
mobile. Phase 5 adds a second back end to the generator — one that compiles
the blueprint's route logic for WebAssembly — and the binding layer that
exports the same logic to Kotlin and Swift, with no second implementation of
either.

**Context**: Phase 4 closed the ecosystem work. `rivet build` writes one
crate whose handler path is axum over a transport-free service layer, and
`rivet dev`, service discovery, the admin panel, and `rivet sync` all build
on that crate. Phase 5 is the first phase whose subject is the *artifact* the
generator produces rather than its runtime topology: the same blueprint must
build for a platform that has no HTTP server, no threads, and no reactor.

The service layer is what makes that possible. `service::*` (pillar 02) is
already transport-free: one async function per route, over JSON values, with
no axum types and no I/O. The WASM target reuses those exact functions, so
pillar 08's claim — "the mobile SDKs contain the same business logic as the
backend, no duplication" — becomes a property of the generator, not a
promise.

Two pillars define the phase:

- `docs/pillars/08-wasm-mobile-sdk-support.md` — the WASM target and the
  UniFFI bindings.
- `docs/pillars/data-structured-under-the-hood.md` — the core stays
  WASM-friendly: no SQLite, no network, no HTTP type in `rivet-core`.

**Non-negotiables**: `rivet-core` stays the IR contract and gains no HTTP,
database, or network dependency. New error codes continue the range after
`E3023`. No `unwrap`/`expect` outside tests. No stubs: a shipped target
answers real requests on the platform it claims, or it is not shipped.

---

## Tasks

### 1. `rivet build --target wasm`

Add `--target native|wasm` to `rivet build`, defaulting to `native`.

The `wasm` target writes a crate that compiles to `wasm32-wasip1` and
answers the blueprint's routes with no HTTP server, no tokio, and no axum:

- The generated crate carries the **same** `mod service` and the **same DTO
  structs** as the native target. The manifest omits axum, tokio, gRPC, and
  every asset and plugin dependency.
- The entry point reads one request as JSON on stdin
  (`{"method": "GET", "path": "/ping"}`), dispatches it to the matching
  `service::*` function, and writes one JSON response
  (`{"status": 200, "body": {...}}`) to stdout. A path no route matches
  answers `404` with a JSON body; a method that matches no route on that
  path answers `405`.
- The service functions are `async fn` but never await in this code base, so
  the entry point needs no reactor. A small hand-written `block_on` (a no-op
  waker and one poll) is the whole executor; state why in a comment.
- Dispatch is a `match` over `(method, path)` rendered at build time. No
  registry, no table, no allocation per request beyond the request itself.

Acceptance: `rivet build --target wasm examples/basic/app.py` writes the
crate, `cargo build --target wasm32-wasip1 --release` succeeds, and
`wasmtime run <module>` with a `GET /ping` request on stdin answers
`{"status":200,"body":{"status":"pong"}}`; an unknown path answers `404`.

### 2. UniFFI bindings for Kotlin and Swift

Generate the bindings from the blueprint, not from a copied interface:

- Add a `[[bin]] uniffi-bindgen` entry point (or a `rivet bindings` command)
  that runs `uniffi::uniffi_bindgen_main()`.
- The generated crate gains a `mobile` module behind a `mobile` feature: a
  `#[uniffi::export]` function per route, over the same `service::*` call,
  so the exported surface and the HTTP surface cannot drift.
- `rivet.toml` grows `[mobile]` with `name`, `namespace`, and `platforms`.

Acceptance: `cargo build --features mobile` and `uniffi-bindgen generate`
produce a Kotlin file and a Swift file for the fixture core, and the Swift
file compiles against it with `swiftc`.

### 3. `rivet mobile init --platforms ios,android`

Scaffold the platform projects around the generated core:

- iOS: an XCFramework-ready Swift package with the generated Swift binding
  and a test that calls one route.
- Android: a Gradle module with the generated Kotlin binding and a JVM test
  that calls one route.

Acceptance: the generated projects build in the platform toolchains in CI
(`xcodebuild` for iOS, `gradle test` for Android).

### 4. The `rust_native_features` flags

Flip each of the four flags in the cross-phase section as its feature lands,
and stop generating the error path it replaces:

- `const_generics`: `List[T, N]` renders `[T; N]` instead of `E2003`.
- `zero_copy_deserialization`: borrowed request bodies via `#[serde(borrow)]`.
- `raii_connections`: pooled connections released at handler exit.
- `compile_time_rbac`: typestate auth so a protected route needs an
  authenticated request type at compile time.

Acceptance: all four flags are true in the example config, and no generator
error path for them remains.

### 5. Docs and tracker

In the commit that lands each feature:

- Finish `docs/pillars/08-wasm-mobile-sdk-support.md`: the target matrix, the
  request protocol, what the wasm crate deliberately omits, and the binding
  surface.
- Record any platform the repository cannot verify here, with the toolchain
  it needs, instead of claiming a green build.
- Tick the matching `docs/ROADMAP.md` phase-5 checkbox and the cross-phase
  feature line.
- Move each finished `tasks/todo.md` line to `tasks/completed.md` with its
  finishing commit hash, in a follow-up commit.

Acceptance Criteria
- `rivet build --target wasm` produces a module that answers the blueprint's
  routes under Wasmtime, and the module carries the same service functions
  as the native crate.
- `rivet build` defaults to the native target and behaves exactly as before.
- UniFFI generates a Kotlin and a Swift binding from the fixture core, and
  each compiles in its platform toolchain.
- `rivet mobile init --platforms ios,android` scaffolds projects that build
  and pass one test each.
- The four `rust_native_features` flags are true in the example config, and
  the generator has no error path left for them.
- `cargo fmt --all -- --check`, `cargo clippy -- -D warnings`, and
  `cargo test --workspace` all pass; `./scripts/gate.sh` is green.

Agent Instructions
1. Follow the existing module layout: target-specific rendering lives beside
   the renderer it extends (`rivet-cli/src/transpiler/rust/wasm.rs`), with
   its unit tests in `wasm/tests.rs` and the split driven by the 400-line
   file ceiling.
2. A unit test on generated text is not proof that the generated crate
   compiles. Verify the wasm target end to end: build it with
   `rustup target add wasm32-wasip1` and run the module under `wasmtime`.
3. Never make the wasm target a copy of the native target. It shares
   `mod service` and the DTO structs; everything else it omits.
4. New error codes continue the range after `E3023`, and each states its
   meaning in its `suggested_fix`.
5. Build JSON by hand from `serde_json::Value`. Never use the `json!` macro,
   `unwrap`, or `expect` outside tests.
6. A platform the repository cannot build here is recorded as blocked with
   the toolchain it needs. Do not ship generator code whose output no
   toolchain in this repository has compiled.
7. `make clean` after the session regenerates crates; tests clean their own
   fixtures through the `ScratchDir` guard.

Output
A blueprint that runs as a compiled axum server, as a WASI module on
Wasmtime, and as a Kotlin or Swift library — from one Python source, with one
implementation of the route logic — all recorded in pillar 08, the roadmap,
and the tracker.
