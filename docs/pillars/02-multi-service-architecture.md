# 2. Multi-Service Architecture

Rivet generates one service layer and two ways to reach it. You pick the
topology in `rivet.toml`; the route logic never changes.

```toml
[transport]
mode = "in_process"   # or "grpc"
grpc_port = 50051     # used when mode = "grpc"
```

```rust
/// Transport-free route logic. The channel calls these functions.
mod service {
    use super::*;

    // GET /ping
    pub async fn ping() -> serde_json::Value {
        rivet_json_obj(vec![("status", serde_json::Value::from("pong"))])
    }
}
```

### The handler module

The axum handlers live inside `mod handlers`, not at the crate root, and the
router reaches them through the module:

```rust
mod handlers {
    use super::*;

    pub async fn ping<C: channel::Channel>(super::State(channel): super::State<C>) -> Result<super::Json<serde_json::Value>, (axum::http::StatusCode, String)> {
        channel.ping()
            .await
            .map(super::Json)
            .map_err(rivet_channel_error)
    }
}

let app = Router::new()
    .route("/ping", get(handlers::ping::<channel::InProcess>))
    .with_state(channel::InProcess);
```

The module is what keeps a handler name out of the crate root. A handler is
a local function, and a local function outranks the `use super::*` glob
import, so a handler named `service` or `String` compiles here.

Two references still reach the crate root, and each is written `super::…`
for the same reason. A handler named `State` would otherwise capture its own
`State(channel)` extractor pattern, and a handler named `Json` would
capture its own response wrapper. An explicit path is not a glob import, so
a local item cannot shadow it.

## Names the generated crate owns

The generated crate is one flat Rust namespace for user-derived names and
generator-owned names. Two rules keep them apart, and
`rivet-core/src/reserved.rs` holds both:

- **Every generator-owned symbol carries the `rivet_` prefix** —
  `rivet_json_obj`, `rivet_channel_error`, `RIVET_DECLARED_PATHS`,
  `mod rivet_fixed_array`, `mod rivet_executor`. A helper added later cannot
  collide with a name a user picks.
- **A DTO may not reuse a name the crate root holds** — the generated
  modules (`service`, `channel`, `admin`, `assets`, `discovery`,
  `handlers`), the entry point `main`, the types the emitter writes
  (`String`, `Vec`, `Json`, `State`, `Router`, `bool`, `i64`, `f64`, and
  `Option`), and the crate paths (`serde`, `serde_json`, `axum`, `tokio`,
  `tracing`, `std`).

The parser rejects a collision with `E1013`, at the offending line, before
any crate is written. Both targets therefore reject the same input and build
the same input, which is the invariant the guard exists to hold.

`Option` is reserved even though it compiles today. The emitter writes
`Option<{base}>` only for an optional field, so reserving it on demand would
make the rule depend on an unrelated field of the DTO. A predictable rule
beats a permissive one.

The rule is scoped by namespace. A **DTO** takes the full set, because a DTO
becomes a crate-root struct. A **handler** takes the prefix alone, because a
handler becomes a function inside `mod handlers`. A **field** takes nothing:
it lives inside its struct, so `pub main: String` inside `pub struct Foo`
compiles.
```

## The channel

An internal channel is the app's route table as typed async methods:

```rust
pub trait Channel {
    fn ping(&self) -> impl std::future::Future<Output = Result<serde_json::Value, String>> + Send;
}
```

Two concrete types implement it. Nothing in the request path looks a
transport up:

- **`InProcess`** calls `service::ping().await` and wraps the result. The
  compiler monomorphizes the call, so the monolith carries a direct call.
- **`Grpc`** serializes the request to JSON, sends one unary call, and
  deserializes the response.

The axum handler is a thin wrapper over whichever channel the project
selected, and the router registers the concrete type:

```rust
async fn ping<C: channel::Channel>(State(channel): State<C>) -> Result<Json<serde_json::Value>, (axum::http::StatusCode, String)> {
    channel.ping()
        .await
        .map(Json)
        .map_err(channel_error)
}

let app = Router::new()
    .route("/ping", get(ping::<channel::InProcess>))
    .with_state(channel::InProcess);
```

A channel failure maps to `502` with the reason, so a broken remote service
is visible instead of silent.

## The gRPC transport

In `grpc` mode the binary serves its channel on `grpc_port` and calls
through it, so one process has both halves of the topology and the HTTP
route path is a real network hop:

```rust
// grpc mode: serve this blueprint's channel, then call through it.
let channel_addr = format!("{host}:50051");
let channel_listener = tokio::net::TcpListener::bind(&channel_addr).await.expect("failed to bind the service channel");
tokio::spawn(async move {
    if let Err(err) = channel::serve(channel_listener).await {
        eprintln!("{err}");
    }
});
let channel = match channel::Grpc::connect(format!("http://{channel_addr}")).await { .. };
```

Deploy it as microservices by running the same blueprint in two processes
and pointing a client process at the other's channel port.

### Why no `protoc`

The gRPC service is hand-written on `tonic::codec`, `tonic::server::Grpc`,
and `tonic::client::Grpc`. Rivet does not run `tonic-build` and does not
depend on `prost`:

- a generated app must build on a machine with only rustc and cargo, and
  `protoc` is an external tool the user may not have;
- Rivet owns both ends of the channel, so the schema is the DSL blueprint,
  not a `.proto` file.

The message is one JSON string, encoded by a codec Rivet writes in the
generated crate (about 60 lines). JSON costs a parse per hop that
protobuf would avoid; the topology switch is the feature here, and the
in-process transport pays nothing for it. The generated manifest pins
`tonic` 0.12 with `default-features = false, features = ["transport"]`.
On tonic 0.14 the `router` feature must be added, because the `transport`
feature no longer implies it.

## Service discovery

`[discovery]` makes the generated app join a service registry, so another
process can find the service it runs:

```toml
[discovery]
backend = "consul"         # or "etcd"
url = "http://127.0.0.1:8500"   # optional; the backend's local port
service_name = "orders-api"     # optional; the project name
service_port = 8080             # optional; the development port
```

A project with no `[discovery]` section registers nowhere and starts as it
always did.

The app registers once at startup, before it serves, and removes itself on
SIGINT. The shutdown is graceful: `main` serves the axum app with
`with_graceful_shutdown`, so an in-flight request finishes before the
process leaves the registry.

| Backend | Registration | Deregistration |
| :--- | :--- | :--- |
| Consul | `PUT /v1/agent/service/register` with the service `ID`, `Name`, and `Port` | `PUT /v1/agent/service/deregister/<name>` |
| etcd | `POST /v3/lease/grant`, then `POST /v3/kv/put` of `/rivet/services/<name>` on that lease | `POST /v3/lease/revoke` |

The etcd lease carries a 60-second TTL, and a keeper task re-grants the
lease every 20 seconds. etcd has no single-request lease refresh, so the
keeper grants a new lease, moves the key onto it, and releases the old one:
the key is never absent while the app runs, and a process that dies without
deregistering leaves a lease that expires on its own.

The client is a small hand-written HTTP/1.1 client in the generated crate:
one connection per request, plain `http://` only. Registering happens once,
so an HTTP stack would cost more than it saves, and a generated app must
build on a machine with only rustc and cargo. An `https://` registry needs
a TLS terminator in front of it.

Registration never stops the app:

- A registry that refuses the connection, refuses the request, or does not
  answer within five seconds prints a warning, and the app serves anyway.
  Every request carries that deadline, connect included, so a registry that
  accepts a connection and never answers cannot hold startup.
- A registry URL that does not start with `http://` is a runtime warning
  for the same reason: the client reports the scheme it cannot speak and
  the app continues without registering.

The build rejects only a configuration it cannot generate for: `rivet build`
fails with `E2005` when the service name cannot go into a registry path or
key — an empty name, a name longer than 128 characters, or one holding a
character outside letters, digits, dot, dash, and underscore.

## Scope

The channel carries unary JSON calls. Streaming, TLS between services,
retries, and load balancing are not implemented. The registry client
registers and deregisters one service; health checks that a registry polls
(Consul's own `check` block, for example) are the registry's job, not the
generated app's.
