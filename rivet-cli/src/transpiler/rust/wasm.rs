//! The WebAssembly target: the same service layer, with no server.
//!
//! `rivet build --target wasm` renders the blueprint's route logic as a WASI
//! command module. The crate it writes carries the **same** DTO structs and
//! the **same** `mod service` as the native target (pillar 02), so a route's
//! business logic has one implementation; everything else is different:
//!
//! - no axum, no tokio, no gRPC, no plugins, and no embedded assets, because
//!   a WASI module has no sockets to bind and no filesystem to serve from;
//! - the module is an edge handler, not a server: it reads one request as
//!   JSON on stdin, dispatches it, and writes one response envelope as JSON
//!   on stdout. An edge host runs one module instance per request.
//! - the dispatch is a `match` rendered at build time, so a request costs one
//!   comparison chain and no allocation beyond the request itself.
//!
//! The service functions are `async` but never await: the parser's subset is
//! literals, request parameters, and one DTO construction, so no route holds
//! I/O and the module needs no reactor. The executor is therefore a no-op
//! waker and one poll, and the module carries no async runtime.
//!
//! A command module is what a WASI host runs directly:
//! `wasmtime run --dir . module.wasm < request.json`.

use super::Codegen;
use super::service;
use crate::config::RivetConfig;
use crate::diagnostic::Diagnostic;
use rivet_core::ir::{RequestSpec, RouteDefinition, ServiceBlueprint};

/// The Rust target the WebAssembly build compiles for.
///
/// `wasm32-wasip1` is the current WASI preview-1 target: a command module the
/// host runs with `wasmtime run`. The component-model target is not used.
pub const WASM_TARGET: &str = "wasm32-wasip1";

/// A ready-to-write WebAssembly crate.
pub struct WasmProject {
    /// The cargo package name, sanitized from the project name.
    pub package_name: String,
    /// The crate source, written to `src/main.rs`.
    pub main_rs: String,
    /// The crate manifest.
    pub cargo_toml: String,
}

/// Render the WebAssembly crate for a blueprint.
pub fn generate_wasm_project(
    blueprint: &ServiceBlueprint,
    config: &RivetConfig,
) -> Result<WasmProject, Diagnostic> {
    super::check_features(config)?;
    let package_name = super::crate_name(&config.project.name);
    let codegen = Codegen {
        structs: &blueprint.structs,
        features: &config.rust_native_features,
    };

    let structs = codegen.render_structs()?;
    let service = service::render_service_module(&codegen, blueprint)?;
    let dispatch = render_dispatch(&codegen, blueprint)?;

    let main_rs = MAIN
        .replace("@@HELPERS@@", super::helpers::SHARED.trim_end())
        .replace("@@STRUCTS@@", structs.trim_end())
        .replace("@@SERVICE@@", service.trim_end())
        .replace("@@DISPATCH@@", &dispatch)
        .replace("@@PATHS@@", &render_paths(blueprint))
        .replace("@@ALLOWED@@", &render_allowed(blueprint));

    Ok(WasmProject {
        cargo_toml: render_manifest(&package_name),
        package_name,
        main_rs,
    })
}

/// The declared paths, as the `DECLARED_PATHS` literal.
fn render_paths(blueprint: &ServiceBlueprint) -> String {
    let mut paths: Vec<String> = Vec::new();
    for route in &blueprint.routes {
        if !paths.contains(&route.path) {
            paths.push(route.path.clone());
        }
    }
    paths
        .iter()
        .map(|path| super::rust_str(path))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The manifest: serde and serde_json, and nothing else.
///
/// A WASI module has no sockets and no threads, so the native target's axum,
/// tokio, gRPC, and asset dependencies are absent by construction rather than
/// by a disabled feature.
fn render_manifest(package_name: &str) -> String {
    format!(
        r#"[workspace]

[package]
name = "{package_name}"
version = "0.1.0"
edition = "2024"

[[bin]]
name = "{package_name}"
path = "src/main.rs"

[dependencies]
serde = {{ version = "1", features = ["derive"] }}
serde_json = "1"

[profile.release]
strip = true
opt-level = "s"
"#
    )
}

/// Render the dispatch: one arm per route, plus the fallbacks.
fn render_dispatch(
    codegen: &Codegen<'_>,
    blueprint: &ServiceBlueprint,
) -> Result<String, Diagnostic> {
    let mut arms = String::new();
    for route in &blueprint.routes {
        arms.push_str(&render_arm(codegen, route)?);
    }
    Ok(arms.trim_end().to_string())
}

/// Render one dispatch arm: parse the body, call the service, serialize the
/// answer.
fn render_arm(codegen: &Codegen<'_>, route: &RouteDefinition) -> Result<String, Diagnostic> {
    let method = route.method.as_str();
    let path = route.path.as_str();
    let name = route.handler_name.as_str();

    // The request: no parameter, or one JSON body of the declared type.
    let (binding, call) = match &route.request {
        RequestSpec::None => (String::new(), format!("service::{name}().await")),
        RequestSpec::Json { var, ty } => {
            let rust_type = codegen.rust_type(ty, false)?;
            // A body that does not match the declared type is the client's
            // error, so it answers 400 rather than a server fault.
            let binding = format!(
                "            let {var}: {rust_type} = match body {{\n                Some(body) => serde_json::from_str(body)\n                    .map_err(|err| (400, format!(\"cannot read the request body: {{err}}\")))?,\n                None => return Err((400, \"this route requires a JSON body\".to_string())),\n            }};\n"
            );
            (binding, format!("service::{name}({var}).await"))
        }
    };

    Ok(format!(
        "        (\"{method}\", \"{path}\") => {{\n{binding}            let value = {call};\n            Ok(serde_json::to_value(value).unwrap_or(serde_json::Value::Null))\n        }}\n"
    ))
}

/// Render the `allowed_methods` function: one arm per declared path.
fn render_allowed(blueprint: &ServiceBlueprint) -> String {
    let mut arms = String::new();
    for route in &blueprint.routes {
        arms.push_str(&format!(
            "        {} => {},\n",
            super::rust_str(&route.path),
            super::rust_str(&allowed_for(blueprint, &route.path))
        ));
    }
    arms
}

/// The methods one path answers, in declaration order, for a `405` answer.
fn allowed_for(blueprint: &ServiceBlueprint, path: &str) -> String {
    let mut methods: Vec<&str> = Vec::new();
    for route in blueprint.routes.iter().filter(|route| route.path == path) {
        let method = route.method.as_str();
        if !methods.contains(&method) {
            methods.push(method);
        }
    }
    methods.join(", ")
}

/// The generated crate.
///
/// A WASI command module: it reads one request from stdin, answers it, and
/// writes one response envelope to stdout. `@@` tokens stand in for a
/// `format!` template, because the module is mostly braces.
const MAIN: &str = r#"//! Generated by rivet. Do not edit; run `rivet build --target wasm` again.
//!
//! An edge handler: one request in on stdin, one response out on stdout. The
//! route logic in `mod service` is the same code the native target compiles.

use std::io::{Read, Write};
@@STRUCTS@@

@@HELPERS@@

@@SERVICE@@

/// Run one service future to completion.
///
/// Every generated route is an `async fn` that never awaits: the parser's
/// subset holds literals, request parameters, and one DTO construction, so a
/// future is ready on its first poll. One poll with a no-op waker is the
/// whole executor, and the module carries no async runtime.
mod executor {
    use std::future::Future;
    use std::pin::pin;
    use std::task::{Context, Poll, Waker};

    /// Run one future to completion.
    ///
    /// A future that returned `Pending` would spin here, which is why the
    /// generated routes hold no I/O: this module has no reactor to make
    /// progress on it. See `docs/pillars/08-wasm-mobile-sdk-support.md`.
    pub(super) fn block_on<F: Future>(future: F) -> F::Output {
        let mut future = pin!(future);
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);
        loop {
            if let Poll::Ready(output) = future.as_mut().poll(&mut context) {
                return output;
            }
        }
    }
}

/// The paths this module serves, so an unknown path answers `404` and a known
/// path with the wrong method answers `405`.
const DECLARED_PATHS: &[&str] = &[@@PATHS@@];

/// The methods one path answers.
fn allowed_methods(path: &str) -> &'static str {
    match path {
@@ALLOWED@@
        _ => "",
    }
}

/// Answer one request: `Ok(body)` for a `200`, or the status and message for
/// a `400`, `404`, or `405`.
///
/// The function is `async` because the service layer is: the executor polls
/// it inside `answer`.
async fn dispatch(
    method: &str,
    path: &str,
    body: Option<&str>,
) -> Result<serde_json::Value, (u16, String)> {
    if !DECLARED_PATHS.contains(&path) {
        return Err((404, format!("no route serves {path}")));
    }
    match (method, path) {
@@DISPATCH@@
        _ => Err((
            405,
            format!(
                "{method} is not served by {path}; it allows {}",
                allowed_methods(path)
            ),
        )),
    }
}

/// Answer an error body: `{"error": "..."}`.
fn json_error(message: &str) -> serde_json::Value {
    let mut object = serde_json::Map::new();
    object.insert(
        "error".to_string(),
        serde_json::Value::String(message.to_string()),
    );
    serde_json::Value::Object(object)
}

/// Read the request, answer it, and write the response envelope.
///
/// The request is `{"method": "GET", "path": "/ping", "body": "{...}"}`, and
/// the response is `{"status": 200, "body": {...}}`. An unreadable request
/// answers `400` rather than exiting: an edge host reads the envelope to
/// decide the HTTP status, so the module always writes one.
fn main() {
    let mut raw = String::new();
    let read = std::io::stdin().read_to_string(&mut raw);
    let (status, body) = match read {
        Ok(_) => answer(&raw),
        Err(err) => (400, json_error(&format!("cannot read the request: {err}"))),
    };

    let mut envelope = serde_json::Map::new();
    envelope.insert("status".to_string(), serde_json::Value::from(status));
    envelope.insert("body".to_string(), body);
    let envelope = serde_json::Value::Object(envelope).to_string();

    let mut stdout = std::io::stdout();
    if stdout.write_all(envelope.as_bytes()).is_err() {
        std::process::exit(1);
    }
}

/// The request body as text.
///
/// A host may send the body either as a JSON string (the raw body) or as a
/// JSON value (the parsed body). Both mean the same request, so a value is
/// re-serialized rather than rejected: answering 400 because a body arrived
/// parsed would blame the client for a shape it cannot know about.
fn body_text(request: &serde_json::Value) -> Option<String> {
    match request.get("body") {
        Some(serde_json::Value::String(text)) => Some(text.clone()),
        Some(value) if !value.is_null() => Some(value.to_string()),
        _ => None,
    }
}

/// Parse one request and answer its envelope body.
fn answer(raw: &str) -> (u16, serde_json::Value) {
    let request: serde_json::Value = match serde_json::from_str(raw) {
        Ok(request) => request,
        Err(err) => return (400, json_error(&format!("cannot read the request: {err}"))),
    };
    let method = request.get("method").and_then(serde_json::Value::as_str);
    let path = request.get("path").and_then(serde_json::Value::as_str);
    let (Some(method), Some(path)) = (method, path) else {
        return (400, json_error("the request names no method or path"));
    };
    let body = body_text(&request);
    match executor::block_on(dispatch(method, path, body.as_deref())) {
        Ok(value) => (200, value),
        Err((status, message)) => (status, json_error(&message)),
    }
}
"#;

#[cfg(test)]
mod tests;
