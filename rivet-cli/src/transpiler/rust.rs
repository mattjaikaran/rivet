//! Rust code generation: the DTO structs, the transport-free service layer,
//! the internal channel, the axum handlers, the router, and a buildable
//! `Cargo.toml`.
//!
//! The parser guarantees the blueprint stays inside the supported subset (one
//! JSON request parameter; a single return of literals, request parameters,
//! or DTO construction). The generator therefore does not re-validate
//! semantics; it renders the IR to Rust syntax.

use crate::config::{RivetConfig, RustNativeFeatures, TransportMode};
use crate::diagnostic::Diagnostic;
use crate::plugin::{self, ResolvedPlugin};
use rivet_core::ir::{
    Expr, FieldDefinition, RequestSpec, RouteDefinition, ServiceBlueprint, Stmt, StructDefinition,
    TypeRef,
};
use rivet_core::reserved;
use std::collections::HashMap;
use std::path::Path;

mod admin;
mod assets;
mod borrow;
mod channel;
mod discovery;
mod expression;
mod handler;
mod helpers;
mod main_file;
mod operation;
mod service;

mod wasm;

pub use assets::AssetEmbedding;
use main_file::{MainBlocks, MainParts, assemble, render_manifest, used_router_fns};
pub use wasm::{WASM_TARGET, generate_wasm_project};

/// A complete, ready-to-write generated crate, with what became of the
/// project's static assets.
pub struct GeneratedProject {
    pub package_name: String,
    pub main_rs: String,
    pub cargo_toml: String,
    /// The static assets compiled into the binary.
    pub assets: AssetEmbedding,
}

/// Reject a `[rust_native_features]` flag the generator does not implement.
///
/// The section advertises what the generator does. A flag set true that the
/// build ignores would make every reader of that file — a person or an agent
/// — believe the crate carries a capability it does not.
pub(super) fn check_features(config: &RivetConfig) -> Result<(), Diagnostic> {
    let Some(name) = config.rust_native_features.unimplemented() else {
        return Ok(());
    };
    Err(Diagnostic::blocker(
        "E2013",
        format!("`{name}` is set in `[rust_native_features]`, and the generator does not implement it yet"),
        format!(
            "set `{name} = false` in `rivet.toml` (the flag advertises what the generator does), then rerun the command"
        ),
    )
    .located("rivet.toml", 1))
}

/// Reject two routes that serve the same method and path.
///
/// The Gauntlet's duplicate rule compares handler *bodies*, so two routes
/// that share a method and a path but differ in body pass it. Each target
/// then fails differently and late: the native router panics at startup with
/// axum's "Overlapping method route", and the wasm dispatch emits two
/// identical match arms, where the first silently wins. Neither is a build
/// error the user can read, so the generator rejects the blueprint.
pub(super) fn check_routes(blueprint: &ServiceBlueprint) -> Result<(), Diagnostic> {
    let mut seen: Vec<(&str, &str, &str)> = Vec::new();
    for route in &blueprint.routes {
        let served = (
            route.method.as_str(),
            route.path.as_str(),
            route.handler_name.as_str(),
        );
        if let Some((method, path, first)) = seen
            .iter()
            .find(|(method, path, _)| (*method, *path) == (served.0, served.1))
            .copied()
        {
            // No `.located`: the generator holds no app path, so a location
            // would name a file that may not exist. The message carries the
            // method, the path, and both handlers, which is what to search
            // for. The Gauntlet's own `E2043` reports lines, and it runs
            // before this check.
            return Err(Diagnostic::blocker(
                "E2014",
                format!(
                    "two routes serve `{method} {path}`: `{first}` and `{}`",
                    route.handler_name
                ),
                format!(
                    "give each route its own method or path, so `{method} {path}` is served once, then rerun the command"
                ),
            ));
        }
        seen.push(served);
    }
    Ok(())
}

/// Render the whole crate from a blueprint, the project configuration, and
/// the project directory that anchors plugin paths.
pub fn generate_project(
    blueprint: &ServiceBlueprint,
    config: &RivetConfig,
    project_dir: &Path,
) -> Result<GeneratedProject, Diagnostic> {
    check_features(config)?;
    check_routes(blueprint)?;
    borrow::check(blueprint, config)?;
    let package_name = crate_name(&config.project.name);
    let plugins = plugin::resolve(config, project_dir)?;
    let mode = config.transport.mode;
    let codegen = Codegen {
        structs: &blueprint.structs,
        features: &config.rust_native_features,
    };

    let host = &config.environments.development.host;
    let port = config.environments.development.port;

    let (channel_type, channel_state) = match mode {
        TransportMode::InProcess => ("channel::InProcess", "channel::InProcess"),
        // The router needs the type; `main` passes the connected value.
        TransportMode::Grpc => ("channel::Grpc", "channel"),
    };
    let structs_block = codegen.render_structs()?;
    let service_block = service::render_service_module(&codegen, blueprint)?;
    let channel_block = channel::render_channel_module(&codegen, blueprint, mode)?;
    let handlers = handler::render_handlers(&codegen, blueprint)?;
    let assets = assets::resolve(config, project_dir);
    let wiring = assets::render(&assets, config.frontend.spa);
    let router = codegen.render_router(blueprint, channel_type);
    let admin = admin::render(blueprint, config.admin.enabled)?;
    let discovery = discovery::render(config)?;
    let parts = MainParts {
        routing: used_router_fns(blueprint, config.admin.enabled).join(", "),
        has_path_params: blueprint
            .routes
            .iter()
            .any(|route| !route.path_params.is_empty()),
        has_query_params: blueprint
            .routes
            .iter()
            .any(|route| !route.query_params.is_empty()),
        host: host.clone(),
        port,
        plugins: render_plugin_installs(&plugins),
        channel_setup: render_channel_setup(config),
        channel_state: channel_state.to_string(),
        assets_fallback: wiring.fallback,
        assets_layer: wiring.compression,
        admin_routes: admin.routes,
        discovery_register: discovery.register,
        discovery_serve: discovery.serve,
    };
    let blocks = MainBlocks {
        structs: &structs_block,
        service: &service_block,
        channel: &channel_block,
        handlers: &handlers,
        router: &router,
        assets: &wiring.module,
        admin: &admin.module,
        discovery: &discovery.module,
    };
    let main_rs = assemble(&blocks, &parts)?;
    let cargo_toml = render_manifest(
        &package_name,
        &plugins,
        mode,
        !wiring.module.is_empty(),
        config.discovery.backend.is_some(),
    );

    Ok(GeneratedProject {
        package_name,
        main_rs,
        cargo_toml,
        assets,
    })
}

/// The setup `main` needs before it builds the router.
///
/// In `grpc` mode the app serves its own channel and connects to it, so the
/// handlers reach the service layer over the wire. In `in_process` mode
/// there is nothing to set up.
fn render_channel_setup(config: &RivetConfig) -> String {
    if config.transport.mode == TransportMode::InProcess {
        return String::new();
    }
    let grpc_port = config.transport.grpc_port;
    format!(
        "    // grpc mode: serve this blueprint's channel, then call through it.\n    let channel_addr = format!(\"{{host}}:{grpc_port}\");\n    let channel_listener = tokio::net::TcpListener::bind(&channel_addr)\n        .await\n        .expect(\"failed to bind the service channel\");\n    tokio::spawn(async move {{\n        if let Err(err) = channel::serve(channel_listener).await {{\n            eprintln!(\"{{err}}\");\n        }}\n    }});\n    let channel = match channel::Grpc::connect(format!(\"http://{{channel_addr}}\")).await {{\n        Ok(channel) => channel,\n        Err(err) => {{\n            eprintln!(\"{{err}}\");\n            std::process::exit(1);\n        }}\n    }};\n\n"
    )
}

/// One monomorphized install call per plugin, in plugin-name order.
///
/// The call names the plugin crate directly, so the compiler resolves the
/// plugin and inlines it. There is no trait object, no name table, and no
/// lookup at run time.
fn render_plugin_installs(plugins: &[ResolvedPlugin]) -> String {
    let installs: Vec<String> = plugins
        .iter()
        .map(|plugin| {
            format!(
                "    // Plugin: {name}\n    let app = {crate_ident}::install(app);",
                name = plugin.name,
                crate_ident = plugin.crate_ident(),
            )
        })
        .collect();
    if installs.is_empty() {
        return String::new();
    }
    format!("\n{}\n", installs.join("\n"))
}

/// Stateless render context. DTO structs are already closed over nested
/// references by the parser, in declaration order.
struct Codegen<'a> {
    structs: &'a [StructDefinition],
    /// The project's `[rust_native_features]` flags. A flag the generator
    /// does not implement stays false, so the config never over-claims.
    features: &'a RustNativeFeatures,
}

/// Per-route render state: the name-to-type environment (parameters plus the
/// locals in scope) and identifier use counts (used to clone a parameter when
/// a handler body references it more than once).
struct Emitter<'a> {
    codegen: &'a Codegen<'a>,
    params: &'a HashMap<String, TypeRef>,
    counts: &'a HashMap<String, usize>,
}

impl<'a> Codegen<'a> {
    // -- DTO structs --------------------------------------------------------

    fn render_structs(&self) -> Result<String, Diagnostic> {
        let mut out = String::new();
        if !self.structs.is_empty() {
            out.push_str("// Request and response DTOs.\n");
            for struct_def in self.structs {
                out.push_str(&self.render_struct(struct_def)?);
            }
        }
        // A fixed-size array field borrows this bridge through
        // `#[serde(with = "fixed_array")]`, so it travels with the structs
        // into both targets.
        if self.features.const_generics && self.structs.iter().any(has_fixed_array) {
            out.push_str(helpers::FIXED_ARRAY);
        }
        Ok(out)
    }

    /// Render one DTO struct.
    ///
    /// A DTO with a `borrowed[str]` field takes [`borrow::LIFETIME`], because
    /// that field is a `&'a str` pointing into the request body.
    fn render_struct(&self, struct_def: &StructDefinition) -> Result<String, Diagnostic> {
        let lifetime = if borrow::has_borrowed_field(struct_def) {
            format!("<{}>", borrow::LIFETIME)
        } else {
            String::new()
        };
        let mut out = format!(
            "#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]\npub struct {}{lifetime} {{\n",
            struct_def.name
        );
        for field in &struct_def.fields {
            let ty = self.field_type(field)?;
            out.push_str(&self.field_attrs(field)?);
            out.push_str(&format!("    pub {}: {ty},\n", field.name));
        }
        out.push_str("}\n\n");
        Ok(out)
    }

    /// The Rust type of one DTO field.
    ///
    /// A borrowed field is a slice of the request body, so it renders as
    /// `&'a str` rather than the owned `String` its [`TypeRef`] names. The
    /// parser rejects `Optional[borrowed[str]]`, so the borrow never needs an
    /// `Option` wrapper.
    fn field_type(&self, field: &FieldDefinition) -> Result<String, Diagnostic> {
        if field.is_borrowed {
            return Ok(format!("&{} str", borrow::LIFETIME));
        }
        self.rust_type(&field.type_ref, field.is_optional)
    }

    /// The serde attributes a field needs.
    ///
    /// serde derives `Serialize` and `Deserialize` for arrays up to 32
    /// elements, so a fixed-size array field takes the generated bridge that
    /// goes through a slice and a `Vec`. The field's Rust type stays `[T; N]`.
    ///
    /// A borrowed field takes `#[serde(borrow)]`, so `Deserialize` reads the
    /// text as a `&'a str` slice of the request body instead of allocating a
    /// `String`.
    fn field_attrs(&self, field: &FieldDefinition) -> Result<String, Diagnostic> {
        let mut attrs = String::new();
        if let Some(size) = fixed_array_len(&field.type_ref) {
            // The bridge is typed over `[T; N]`, so an `Option<[T; N]>` would
            // hand it the wrong shape and the generated crate would fail to
            // compile. Reject the combination where the user can see it.
            if field.is_optional {
                return Err(Diagnostic::blocker(
                    "E2012",
                    format!(
                        "`Optional[List[_, {size}]]` is not generated yet: a fixed-size array field cannot be optional"
                    ),
                    "drop the `Optional[...]` wrapper, or declare the field as a plain `List[...]` so an empty value is legal",
                )
                .located("<generated>", 1));
            }
            attrs.push_str(&format!(
                "    #[serde(with = \"{}fixed_array\")]\n",
                reserved::PREFIX
            ));
        }
        if field.is_optional {
            attrs.push_str("    #[serde(default, skip_serializing_if = \"Option::is_none\")]\n");
        }
        if field.is_borrowed {
            attrs.push_str("    #[serde(borrow)]\n");
        }
        Ok(attrs)
    }

    /// Map an IR type to a Rust type name.
    fn rust_type(&self, ty: &TypeRef, is_optional: bool) -> Result<String, Diagnostic> {
        let base = match ty {
            TypeRef::String => "String".to_string(),
            TypeRef::Bool => "bool".to_string(),
            TypeRef::Int => "i64".to_string(),
            TypeRef::Float => "f64".to_string(),
            TypeRef::Json => "serde_json::Value".to_string(),
            TypeRef::Array { element, len: None } => {
                format!("Vec<{}>", self.rust_type(element, false)?)
            }
            // A fixed-size array needs the `const_generics` opt-in: without
            // it the generator reports the same blocker it always did, so a
            // project that has not opted in sees no change.
            TypeRef::Array {
                element,
                len: Some(size),
            } if self.features.const_generics => {
                format!("[{}; {size}]", self.rust_type(element, false)?)
            }
            TypeRef::Array {
                len: Some(size), ..
            } => {
                return Err(Diagnostic::blocker(
                    "E2003",
                    format!("fixed-size arrays (List[T, {size}]) need the `const_generics` opt-in"),
                    "set `const_generics = true` in the `[rust_native_features]` section of `rivet.toml`, or use a plain `List[T]`, then rerun the command",
                )
                .located("<generated>", 1));
            }
            // A borrowed DTO is written with an elided lifetime: every legal
            // position is a parameter or a local, where the borrow comes from
            // the request body the caller holds.
            TypeRef::Named(name) if self.is_borrowed_dto(name) => format!("{name}<'_>"),
            TypeRef::Named(name) => name.clone(),
        };
        Ok(if is_optional {
            format!("Option<{base}>")
        } else {
            base
        })
    }

    /// Register every route with the concrete channel the config selected,
    /// so the compiler monomorphizes each handler for that transport.
    ///
    /// The path names [`reserved::HANDLERS_MODULE`] rather than the crate
    /// root, because that is where the handlers live. A handler name
    /// therefore never reaches the crate root, where it could collide with a
    /// generated module, helper, or import.
    fn render_router(&self, blueprint: &ServiceBlueprint, channel_type: &str) -> String {
        let mut lines = Vec::new();
        for route in &blueprint.routes {
            let router_fn = route.method.axum_router_fn();
            lines.push(format!(
                "        .route({}, {router_fn}({}::{}::<{channel_type}>))",
                rust_str(&route.path),
                reserved::HANDLERS_MODULE,
                route.handler_name
            ));
        }
        lines.join("\n")
    }

    fn find_struct(&self, name: &str) -> Result<&StructDefinition, Diagnostic> {
        self.structs.iter().find(|s| s.name == name).ok_or_else(|| {
            Diagnostic::blocker(
                "E2002",
                format!("DTO `{name}` is not part of the blueprint"),
                format!("declare the `{name}` class in the app module, or point the route's response type at a DTO that exists, then rerun the command"),
            )
            .located("<generated>", 1)
        })
    }

    /// Whether the DTO `name` borrows from the request body.
    fn is_borrowed_dto(&self, name: &str) -> bool {
        self.structs
            .iter()
            .any(|struct_def| struct_def.name == name && borrow::has_borrowed_field(struct_def))
    }

    /// Whether the type `ty` carries a borrow, so a handler that extracts it
    /// must take the raw body instead of `Json`.
    ///
    /// An array counts: `Vec<Note<'_>>` borrows from the same buffer, and
    /// axum's `Json` extractor requires `DeserializeOwned` either way.
    fn borrows(&self, ty: &TypeRef) -> bool {
        match ty {
            TypeRef::Named(name) => self.is_borrowed_dto(name),
            TypeRef::Array { element, .. } => self.borrows(element),
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------
// Free helpers
// ---------------------------------------------------------------------------

/// The parameter types of a route, keyed by parameter name: the path
/// parameters in path order, then the query parameters, then the JSON body
/// parameter.
fn param_types(route: &RouteDefinition) -> HashMap<String, TypeRef> {
    let mut params = HashMap::new();
    for param in &route.path_params {
        params.insert(param.name.clone(), param.ty.clone());
    }
    for param in &route.query_params {
        params.insert(param.name.clone(), param.ty.clone());
    }
    if let RequestSpec::Json { var, ty } = &route.request {
        params.insert(var.clone(), ty.clone());
    }
    params
}

/// Count identifier uses across every expression of a route's body.
fn count_idents_in(route: &RouteDefinition) -> HashMap<String, usize> {
    fn visit(expr: &Expr, counts: &mut HashMap<String, usize>) {
        match expr {
            Expr::Ident(name) => *counts.entry(name.clone()).or_insert(0) += 1,
            Expr::Array(items) => items.iter().for_each(|item| visit(item, counts)),
            Expr::Object(entries) => entries.iter().for_each(|(_, item)| visit(item, counts)),
            Expr::Construct { args, .. } => args.iter().for_each(|(_, item)| visit(item, counts)),
            Expr::Binary { left, right, .. } => {
                visit(left, counts);
                visit(right, counts);
            }
            Expr::Not(operand) => visit(operand, counts),
            _ => {}
        }
    }
    let mut counts = HashMap::new();
    for stmt in Stmt::walk(&route.body) {
        match stmt {
            Stmt::Return(expr) => visit(expr, &mut counts),
            Stmt::Assign { value, .. } => visit(value, &mut counts),
            // The branch bodies are already part of the walk; only the
            // conditions are not, because they guard the branches rather than
            // being statements of them.
            Stmt::If { branches, .. } => {
                for (cond, _) in branches {
                    visit(cond, &mut counts);
                }
            }
        }
    }
    counts
}

/// Sanitize a project name into a valid Cargo package name.
pub(crate) fn crate_name(name: &str) -> String {
    let base = if name.is_empty() {
        "app".to_string()
    } else {
        name.to_string()
    };
    let sanitized: String = base
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let mut sanitized = sanitized.trim_matches('-').to_string();
    if sanitized.is_empty() {
        sanitized = "app".to_string();
    } else if !sanitized.starts_with(|c: char| c.is_ascii_alphabetic()) {
        sanitized = format!("app-{sanitized}");
    }
    sanitized
}

/// Render a string as a Rust string literal.
fn rust_str(value: &str) -> String {
    format!("{value:?}")
}

/// The declared length of a fixed-size array type, when it has one.
fn fixed_array_len(ty: &TypeRef) -> Option<usize> {
    match ty {
        TypeRef::Array {
            len: Some(size), ..
        } => Some(*size),
        _ => None,
    }
}

/// Whether a DTO declares a fixed-size array field, so the generated crate
/// needs the serde bridge for one.
fn has_fixed_array(struct_def: &StructDefinition) -> bool {
    struct_def
        .fields
        .iter()
        .any(|field| fixed_array_len(&field.type_ref).is_some())
}

fn expr_kind(expr: &Expr) -> &'static str {
    match expr {
        Expr::Null => "null",
        Expr::Bool(_) => "bool",
        Expr::Int(_) => "int",
        Expr::Float(_) => "float",
        Expr::Str(_) => "string",
        Expr::Array(_) => "list",
        Expr::Object(_) => "dict",
        Expr::Ident(_) => "parameter reference",
        Expr::Construct { .. } => "constructor call",
        Expr::Binary { .. } => "binary operation",
        Expr::Not(_) => "negation",
    }
}

#[cfg(test)]
mod tests;
