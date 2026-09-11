# 8. WASM & Mobile SDK Export

Rivet compiles one blueprint to three artifacts: a native axum server
(`rivet build`), a WASI module (`rivet build --target wasm`), and a native
library for Kotlin and Swift (`rivet mobile init`). The route logic has one
implementation in every case.

## The native server

The default target: an axum binary, described in pillar 02 (the transport
switch) and pillar 03 (the embedded frontend build).

## The WASI module

`rivet build --target wasm` writes `generated-wasm/` and compiles it to
`wasm32-wasip1`:

```bash
rivet build --target wasm app.py
# Parsed 2 routes and wrote the WASI crate to generated-wasm
# Module: generated-wasm/target/wasm32-wasip1/release/my-api.wasm
```

The crate carries the **same** DTO structs and the **same** `mod service` as
the native target, so a route's business logic has one implementation.
Everything that needs a server is absent by construction, not by a disabled
feature:

| The native target has | The WASI module has |
| :--- | :--- |
| axum handlers and a router | one `rivet_dispatch` match rendered at build time |
| tokio, with worker threads and a reactor | one poll with a no-op waker |
| gRPC and the internal channel | a direct call to `service::*` |
| plugins, embedded assets, discovery | nothing: a module has no sockets and no filesystem |
| a long-running process | one request per module run |

The module is one namespace, so every generator-owned symbol there carries
the `rivet_` prefix: `rivet_dispatch`, `rivet_answer`, `rivet_body_text`,
`rivet_json_error`, `rivet_allowed_methods`, `RIVET_DECLARED_PATHS`,
`mod rivet_executor`, and the shared `rivet_json_obj` and
`rivet_json_number`. A DTO name cannot collide with one.

The dispatch reaches the route logic through the module
(`service::create_order(request).await`), so a handler name never lands in
the module's namespace either. Read
[pillar 02](02-multi-service-architecture.md) for the reserved-name rule and
the `E1013` diagnostic that holds it.

### The request protocol

A module is an edge handler, not a server: the host runs one instance per
request, so the module reads one request from stdin and writes one envelope
to stdout.

```json
{"method": "GET", "path": "/ping"}
```

```json
{"status": 200, "body": {"status": "pong"}}
```

| Request | Answer |
| :--- | :--- |
| A declared route whose body matches its type | `200`, with the route's own value |
| A path no route declares | `404`, with `{"error": "..."}` |
| A declared path with a method it does not serve | `405`, naming the methods it allows |
| A body that does not match the declared type, or no body where one is required | `400` |
| A request the module cannot read | `400` |

The `body` field carries the raw request body as a **JSON string**, not a
nested object. The module parses that text straight into the route's declared
type, so a `dict` route receives its JSON unchanged:

```json
{"method": "POST", "path": "/echo", "body": "{\"hello\": \"world\"}"}
```

```json
{"status": 200, "body": {"echo": {"hello": "world"}}}
```

Run it with a WASI host:

```bash
echo '{"method":"GET","path":"/ping"}' | wasmtime run <module>.wasm
```

The module exits `0` and always writes one envelope, because an edge host
reads the envelope to decide the HTTP status.

### Why the module has no reactor

Every generated route is an `async fn` that never awaits: the parser's subset
is literals, request parameters, and one DTO construction, so no route holds
I/O. A future is therefore ready on its first poll, and the executor is one
poll with `Waker::noop()` — no tokio, no threads, and no runtime to carry.
The `block_on` loop would spin on a future that returned `Pending`, which is
why a route that awaited would have to arrive with a reactor.

### What the module deliberately omits

- **No WASI sockets.** A WASI module creates no socket, so it cannot serve
  HTTP itself. The host owns the listener and calls the module per request.
- **No `[frontend] dist`.** A module has no filesystem to serve from, so the
  embedded asset pipeline does not apply. `rivet build --target wasm` ignores
  the section.
- **No plugins and no discovery.** Both compose into a router or register a
  long-running service; neither exists in a per-request module.

## Mobile bindings

`rivet mobile init --platforms ios,android` generates bindings over the same
`service::*` layer through UniFFI:

- Kotlin (Android)
- Swift (iOS)
- TypeScript (React Native) — **deferred**: this project finishes the Python
  front end first, and the TypeScript binding waits with it. See the scope
  section in `tasks/todo.md`.

The bindings wrap the same functions the HTTP handlers call, so the mobile
SDK and the server cannot drift: one blueprint, one implementation of the
route logic.

## Fixed-size arrays

`[rust_native_features] const_generics = true` renders a `List[T, N]` DTO
field as a fixed-size Rust array:

```python
class Embedding:
    values: List[float, 768]
```

```rust
#[serde(with = "fixed_array")]
pub values: [f64; 768],
```

Without the flag the generator keeps the `E2003` blocker, whose fix names the
opt-in. The flag is the switch, so a config never advertises a capability the
generator does not implement.

serde derives `Serialize` and `Deserialize` for arrays up to 32 elements, so
a larger array borrows a generated bridge: serializing goes through a slice,
and deserializing builds the array from a `Vec` and rejects a length that
does not match the declaration. The bridge is emitted only when a DTO
declares such a field, and both targets carry it, so a 768-element embedding
round-trips as a real array with no `Vec` in the struct.

### Toolchain status

The WASM target is verifiable in this repository: `rustup target add
wasm32-wasip1` and `wasmtime` are installed, and the gate runs the generated
module on its real target.

The mobile bindings are not yet verifiable here. They need a JDK, `kotlinc`,
the Android SDK, `uniffi-bindgen`, and Xcode's command-line tools. Until
those are present, the mobile lines stay open in the tracker with the
toolchain each one needs recorded, rather than shipping generator output that
no toolchain in this repository has compiled.
