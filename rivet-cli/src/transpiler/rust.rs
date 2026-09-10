//! Rust code generation: the DTO structs, the transport-free service layer,
//! the internal channel, the axum handlers, the router, and a buildable
//! `Cargo.toml`.
//!
//! The parser guarantees the blueprint stays inside the supported subset (one
//! JSON request parameter; a single return of literals, request parameters,
//! or DTO construction). The generator therefore does not re-validate
//! semantics; it renders the IR to Rust syntax.

use crate::config::{RivetConfig, TransportMode};
use crate::diagnostic::Diagnostic;
use crate::plugin::{self, ResolvedPlugin};
use rivet_core::ir::{
    Expr, FieldDefinition, RequestSpec, RouteDefinition, ServiceBlueprint, StructDefinition,
    TypeRef,
};
use std::collections::HashMap;
use std::path::Path;

mod admin;
mod assets;
mod channel;
mod discovery;
mod handler;
mod main_file;
mod service;

pub use assets::AssetEmbedding;
use main_file::{MainBlocks, MainParts, assemble, render_manifest, used_router_fns};

/// A complete, ready-to-write generated crate, with what became of the
/// project's static assets.
pub struct GeneratedProject {
    pub package_name: String,
    pub main_rs: String,
    pub cargo_toml: String,
    /// The static assets compiled into the binary.
    pub assets: AssetEmbedding,
}

/// Render the whole crate from a blueprint, the project configuration, and
/// the project directory that anchors plugin paths.
pub fn generate_project(
    blueprint: &ServiceBlueprint,
    config: &RivetConfig,
    project_dir: &Path,
) -> Result<GeneratedProject, Diagnostic> {
    let package_name = crate_name(&config.project.name);
    let plugins = plugin::resolve(config, project_dir)?;
    let mode = config.transport.mode;
    let codegen = Codegen {
        structs: &blueprint.structs,
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
}

/// Per-route render state: parameter types plus identifier use counts (used
/// to clone a parameter when a handler body references it more than once).
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
        Ok(out)
    }

    fn render_struct(&self, struct_def: &StructDefinition) -> Result<String, Diagnostic> {
        let mut out = format!(
            "#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]\npub struct {} {{\n",
            struct_def.name
        );
        for field in &struct_def.fields {
            let ty = self.rust_type(&field.type_ref, field.is_optional)?;
            let attrs = if field.is_optional {
                "    #[serde(default, skip_serializing_if = \"Option::is_none\")]\n"
            } else {
                ""
            };
            out.push_str(&format!("{attrs}    pub {}: {ty},\n", field.name));
        }
        out.push_str("}\n\n");
        Ok(out)
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
            TypeRef::Array {
                len: Some(size), ..
            } => {
                return Err(Diagnostic::blocker(
                    "E2003",
                    format!("fixed-size arrays (List[T, {size}]) are not generated yet; const generics arrive with the zero-copy phase"),
                    "use a plain `List[T]` without a size until the zero-copy phase generates fixed-size arrays",
                )
                .located("<generated>", 1));
            }
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
    fn render_router(&self, blueprint: &ServiceBlueprint, channel_type: &str) -> String {
        let mut lines = Vec::new();
        for route in &blueprint.routes {
            let router_fn = route.method.axum_router_fn();
            lines.push(format!(
                "        .route({}, {router_fn}({}::<{channel_type}>))",
                rust_str(&route.path),
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
}

impl Emitter<'_> {
    /// Render a named (DTO) response: a construction or a parameter.
    fn render_named(
        &self,
        expr: &Expr,
        struct_def: &StructDefinition,
    ) -> Result<String, Diagnostic> {
        match expr {
            Expr::Construct { args, .. } => self.render_struct_expr(struct_def, args),
            Expr::Ident(var) => Ok(self.owned_ident(var)),
            _ => Err(Diagnostic::blocker(
                "E2002",
                format!(
                    "handler must return a `{}` construction or a parameter of that type",
                    struct_def.name
                ),
                format!(
                    "return the `{}` DTO as a construction, or return a request parameter of type `{}`",
                    struct_def.name, struct_def.name
                ),
            )
            .located("<generated>", 1)),
        }
    }

    /// Render a struct literal for a DTO construction.
    fn render_struct_expr(
        &self,
        struct_def: &StructDefinition,
        args: &[(String, Expr)],
    ) -> Result<String, Diagnostic> {
        let mut fields = String::new();
        for field in &struct_def.fields {
            let value = match args.iter().find(|(name, _)| name == &field.name) {
                Some((_, expr)) => self.render_field(expr, field)?,
                None if field.is_optional => "None".to_string(),
                None => {
                    return Err(Diagnostic::blocker(
                        "E2002",
                        format!(
                            "missing required field `{}` for `{}`",
                            field.name, struct_def.name
                        ),
                        format!(
                            "add the missing `{}=...` argument to the `{}` construction in the handler body",
                            field.name, struct_def.name
                        ),
                    )
                    .located("<generated>", 1));
                }
            };
            fields.push_str(&format!("        {}: {value},\n", field.name));
        }
        Ok(format!("{} {{\n{fields}    }}", struct_def.name))
    }

    /// Render one field value with the exact Rust type of the field.
    fn render_field(&self, expr: &Expr, field: &FieldDefinition) -> Result<String, Diagnostic> {
        if field.is_optional {
            if matches!(expr, Expr::Null) {
                return Ok("None".to_string());
            }
            let value = self.render_typed(expr, &field.type_ref)?;
            return Ok(format!("Some({value})"));
        }
        self.render_typed(expr, &field.type_ref)
    }

    /// Render an expression into the Rust type named by `ty`.
    fn render_typed(&self, expr: &Expr, ty: &TypeRef) -> Result<String, Diagnostic> {
        match ty {
            TypeRef::Json => self.render_value(expr),
            TypeRef::String => match expr {
                Expr::Str(value) => Ok(format!("{}.to_owned()", rust_str(value))),
                Expr::Ident(var) => Ok(self.owned_ident(var)),
                _ => Err(self.type_error(expr, ty)),
            },
            TypeRef::Int => match expr {
                Expr::Int(value) => Ok(format!("{value}i64")),
                Expr::Ident(var) => Ok(var.clone()), // i64 is Copy
                _ => Err(self.type_error(expr, ty)),
            },
            TypeRef::Float => match expr {
                Expr::Int(value) => Ok(value.to_string()), // literal widens to f64
                Expr::Float(value) => Ok(value.to_string()),
                Expr::Ident(var) => match self.params.get(var) {
                    Some(TypeRef::Int) => Ok(format!("{var} as f64")),
                    _ => Ok(var.clone()),
                },
                _ => Err(self.type_error(expr, ty)),
            },
            TypeRef::Bool => match expr {
                Expr::Bool(value) => Ok(value.to_string()),
                Expr::Ident(var) => Ok(var.clone()),
                _ => Err(self.type_error(expr, ty)),
            },
            TypeRef::Array { element, .. } => match expr {
                Expr::Array(items) => {
                    let rendered = items
                        .iter()
                        .map(|item| self.render_typed(item, element))
                        .collect::<Result<Vec<_>, _>>()?;
                    Ok(format!("vec![{}]", rendered.join(", ")))
                }
                Expr::Ident(var) => Ok(self.owned_ident(var)),
                _ => Err(self.type_error(expr, ty)),
            },
            TypeRef::Named(name) => match expr {
                Expr::Construct { args, .. } => {
                    let struct_def = self.codegen.find_struct(name)?;
                    self.render_struct_expr(struct_def, args)
                }
                Expr::Ident(var) => Ok(self.owned_ident(var)),
                _ => Err(self.type_error(expr, ty)),
            },
        }
    }

    /// Render an expression of type `serde_json::Value`.
    fn render_value(&self, expr: &Expr) -> Result<String, Diagnostic> {
        match expr {
            Expr::Null => Ok("serde_json::Value::Null".to_string()),
            Expr::Bool(value) => Ok(format!("serde_json::Value::Bool({value})")),
            Expr::Int(value) => Ok(format!("serde_json::Value::from({value}i64)")),
            Expr::Float(value) => Ok(format!("json_number({value})")),
            Expr::Str(value) => Ok(format!("serde_json::Value::from({})", rust_str(value))),
            Expr::Array(items) => {
                let rendered = items
                    .iter()
                    .map(|item| self.render_value(item))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(format!(
                    "serde_json::Value::Array(vec![{}])",
                    rendered.join(", ")
                ))
            }
            Expr::Object(entries) => {
                let rendered = entries
                    .iter()
                    .map(|(key, value)| {
                        Ok(format!(
                            "({}, {})",
                            rust_str(key),
                            self.render_value(value)?
                        ))
                    })
                    .collect::<Result<Vec<_>, Diagnostic>>()?;
                Ok(format!("json_obj(vec![{}])", rendered.join(", ")))
            }
            Expr::Ident(var) => match self.params.get(var) {
                Some(TypeRef::Json) => Ok(self.owned_ident(var)),
                Some(_) => {
                    // Borrow and serialize: works for DTOs and primitives and
                    // is safe under repeated use.
                    Ok(format!(
                        "serde_json::to_value(&{var}).expect(\"request value serialization cannot fail\")"
                    ))
                }
                None => Err(Diagnostic::blocker(
                    "E2002",
                    format!("`{var}` is not a parameter of this handler"),
                    format!("make `{var}` a request parameter of the handler, or replace the reference with a literal"),
                )
                .located("<generated>", 1)),
            },
            Expr::Construct { .. } => Err(Diagnostic::blocker(
                "E2002",
                "DTO construction nested inside a JSON value is not supported yet",
                "return the DTO construction as the handler's declared response type instead of nesting it inside the JSON value",
            )
            .located("<generated>", 1)),
        }
    }

    /// A parameter of a non-`Copy` type is moved on its first use; render a
    /// clone on every use when the handler body references it more than once.
    fn owned_ident(&self, var: &str) -> String {
        if self.counts.get(var).copied().unwrap_or(0) > 1 {
            format!("{var}.clone()")
        } else {
            var.to_string()
        }
    }

    fn type_error(&self, expr: &Expr, ty: &TypeRef) -> Diagnostic {
        Diagnostic::blocker(
            "E2002",
            format!(
                "a {} value cannot satisfy a field of type `{}`",
                expr_kind(expr),
                type_label(ty)
            ),
            format!(
                "return a value of the field's declared type `{}` (a literal, a request parameter of that type, or a DTO construction)",
                type_label(ty)
            ),
        )
        .located("<generated>", 1)
    }
}

// ---------------------------------------------------------------------------
// Free helpers
// ---------------------------------------------------------------------------

/// The request parameter types of a route, keyed by parameter name.
fn param_types(route: &RouteDefinition) -> HashMap<String, TypeRef> {
    match &route.request {
        RequestSpec::Json { var, ty } => HashMap::from([(var.clone(), ty.clone())]),
        RequestSpec::None => HashMap::new(),
    }
}

/// Count identifier uses across all returns of a route.
fn count_idents_in(route: &RouteDefinition) -> HashMap<String, usize> {
    fn visit(expr: &Expr, counts: &mut HashMap<String, usize>) {
        match expr {
            Expr::Ident(name) => *counts.entry(name.clone()).or_insert(0) += 1,
            Expr::Array(items) => items.iter().for_each(|item| visit(item, counts)),
            Expr::Object(entries) => entries.iter().for_each(|(_, item)| visit(item, counts)),
            Expr::Construct { args, .. } => args.iter().for_each(|(_, item)| visit(item, counts)),
            _ => {}
        }
    }
    let mut counts = HashMap::new();
    for expr in &route.returns {
        visit(expr, &mut counts);
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
    }
}

fn type_label(ty: &TypeRef) -> String {
    match ty {
        TypeRef::String => "str".to_string(),
        TypeRef::Bool => "bool".to_string(),
        TypeRef::Int => "int".to_string(),
        TypeRef::Float => "float".to_string(),
        TypeRef::Json => "dict".to_string(),
        TypeRef::Array { .. } => "list".to_string(),
        TypeRef::Named(name) => name.clone(),
    }
}

#[cfg(test)]
mod tests;
