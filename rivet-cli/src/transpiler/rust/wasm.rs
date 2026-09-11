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
//! - the dispatch is a route-by-route segment comparison rendered at build
//!   time, so a request costs one comparison chain and no work beyond the
//!   request's own segments.
//!
//! The service functions are `async` but never await: the parser's subset is
//! literals, request parameters, and one DTO construction, so no route holds
//! I/O and the module needs no reactor. The executor is therefore a no-op
//! waker and one poll, and the module carries no async runtime.
//!
//! A command module is what a WASI host runs directly:
//! `wasmtime run --dir . module.wasm < request.json`.

mod template;

use super::{Codegen, service};
use crate::config::RivetConfig;
use crate::diagnostic::Diagnostic;
use rivet_core::ir::{RequestSpec, RouteDefinition, ServiceBlueprint, TypeRef};
use template::MAIN;

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
    super::check_routes(blueprint)?;
    super::borrow::check(blueprint, config)?;
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

/// The declared paths, deduplicated, in blueprint order.
fn declared_paths(blueprint: &ServiceBlueprint) -> Vec<String> {
    let mut paths: Vec<String> = Vec::new();
    for route in &blueprint.routes {
        if !paths.contains(&route.path) {
            paths.push(route.path.clone());
        }
    }
    paths
}

/// The declared paths, as the `RIVET_DECLARED_PATHS` literal.
fn render_paths(blueprint: &ServiceBlueprint) -> String {
    declared_paths(blueprint)
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

/// Render one dispatch arm: match the path segments, bind every `{name}`
/// placeholder after percent-decoding, then call the service.
fn render_arm(codegen: &Codegen<'_>, route: &RouteDefinition) -> Result<String, Diagnostic> {
    let method = route.method.as_str();
    let path = route.path.as_str();
    let name = route.handler_name.as_str();

    // The segment index of each `{name}` placeholder, in path order. The
    // parser fills `path_params` in the same order, so the two lists line up.
    let placeholders: Vec<usize> = path
        .split('/')
        .enumerate()
        .filter(|(_, segment)| {
            segment.len() >= 2 && segment.starts_with('{') && segment.ends_with('}')
        })
        .map(|(index, _)| index)
        .collect();
    if placeholders.len() != route.path_params.len() {
        return Err(Diagnostic::blocker(
            "E2017",
            format!(
                "`{path}` names {} path parameters but `{name}` declares {}",
                placeholders.len(),
                route.path_params.len()
            ),
            format!("give `{name}` one parameter per `{{name}}` placeholder in `{path}`"),
        ));
    }

    // Bind each parameter from its own source, in the order the handler
    // declares them, so the call below passes them the same way. Every
    // binding is one `let` whose initializer reads the segment or the query
    // directly; no value is stored under a second name, so no route
    // parameter can be shadowed by the generator's own bookkeeping.
    //
    // A value that is missing or does not parse is the client's error, so it
    // answers 400 naming the parameter. An unknown extra query key is never
    // looked up, so it is ignored.
    let mut bindings = String::new();
    let mut args: Vec<String> = Vec::new();
    for (segment_index, param) in placeholders.iter().zip(&route.path_params) {
        let rust_type = codegen.rust_type(&param.ty, false)?;
        let arg = param.name.as_str();
        let segment = format!("rivet_percent_decode(segments[{segment_index}])");
        let binding = if param.ty == TypeRef::Int {
            // The message repeats the decoded segment, which costs a second
            // decode on the error path alone: the request is malformed and
            // the answer is a 400, so the work never reaches a served call.
            format!(
                "            let {arg}: {rust_type} = match {segment}.parse() {{\n                Ok(value) => value,\n                Err(_) => return Err((400, format!(\"`{{}}` is not a valid `{arg}`\", {segment}))),\n            }};\n"
            )
        } else {
            format!("            let {arg}: {rust_type} = {segment};\n")
        };
        bindings.push_str(&binding);
        args.push(arg.to_string());
    }
    for param in &route.query_params {
        let rust_type = codegen.rust_type(&param.ty, false)?;
        let arg = param.name.as_str();
        let found = format!("rivet_query_value(query, \"{arg}\")");
        let missing = format!("\"the query parameter `{arg}` is missing\"");
        let binding = if param.ty == TypeRef::String {
            format!(
                "            let {arg}: {rust_type} = match {found} {{\n                Some(value) => value,\n                None => return Err((400, {missing}.to_string())),\n            }};\n"
            )
        } else {
            let label = param.ty.label();
            format!(
                "            let {arg}: {rust_type} = match {found} {{\n                Some(value) => match value.parse() {{\n                    Ok(value) => value,\n                    Err(_) => return Err((400, format!(\"the query parameter `{arg}` must parse as {label}, got `{{value}}`\"))),\n                }},\n                None => return Err((400, {missing}.to_string())),\n            }};\n"
            )
        };
        bindings.push_str(&binding);
        args.push(arg.to_string());
    }

    // The body parameter, deserialized from the request text.
    if let RequestSpec::Json { var, ty } = &route.request {
        let rust_type = codegen.rust_type(ty, false)?;
        bindings.push_str(&format!(
            "            let {var}: {rust_type} = match body {{\n                Some(body) => serde_json::from_str(body)\n                    .map_err(|err| (400, format!(\"cannot read the request body: {{err}}\")))?,\n                None => return Err((400, \"this route requires a JSON body\".to_string())),\n            }};\n"
        ));
        args.push(var.clone());
    }

    let call = if args.is_empty() {
        format!("service::{name}()")
    } else {
        format!("service::{name}({})", args.join(", "))
    };

    Ok(format!(
        "    if rivet_path_matches(&segments, {}) {{\n        known = true;\n        if method == {:?} {{\n{}            let value = {}.await;\n            return Ok(serde_json::to_value(value).unwrap_or(serde_json::Value::Null));\n        }}\n    }}\n",
        super::rust_str(path),
        method,
        bindings,
        call
    ))
}

/// Render the `rivet_allowed_methods` function: one arm per declared path.
fn render_allowed(blueprint: &ServiceBlueprint) -> String {
    let mut arms = String::new();
    for path in declared_paths(blueprint) {
        arms.push_str(&format!(
            "                {} => {},\n",
            super::rust_str(&path),
            super::rust_str(&allowed_for(blueprint, &path))
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

#[cfg(test)]
mod query_tests;
#[cfg(test)]
mod tests;
