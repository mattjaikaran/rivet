//! Rust code generation: render DTO structs, axum handlers, the router, and
//! a buildable `Cargo.toml`.
//!
//! The parser guarantees the blueprint stays inside the supported subset (one
//! JSON request parameter; a single return of literals, request parameters,
//! or DTO construction). The generator therefore does not re-validate
//! semantics; it renders the IR to Rust syntax.

use crate::config::RivetConfig;
use crate::diagnostic::Diagnostic;
use rivet_core::ir::{
    Expr, FieldDefinition, RequestSpec, ResponseSpec, RouteDefinition, ServiceBlueprint,
    StructDefinition, TypeRef,
};
use std::collections::HashMap;

/// A complete, ready-to-write generated crate.
pub struct GeneratedProject {
    pub package_name: String,
    pub main_rs: String,
    pub cargo_toml: String,
}

/// Render the whole crate from a blueprint and project configuration.
pub fn generate_project(
    blueprint: &ServiceBlueprint,
    config: &RivetConfig,
) -> Result<GeneratedProject, Diagnostic> {
    let package_name = crate_name(&config.project.name);
    let codegen = Codegen {
        structs: &blueprint.structs,
    };
    let host = &config.environments.development.host;
    let port = config.environments.development.port;

    let structs_block = codegen.render_structs()?;
    let handlers = codegen.render_handlers(blueprint)?;
    let router = codegen.render_router(blueprint);
    let parts = MainParts {
        routing: used_router_fns(blueprint).join(", "),
        host: host.clone(),
        port,
    };
    let main_rs = assemble(&structs_block, &handlers, &router, &parts)?;

    let cargo_toml = format!(
        r#"[workspace]

[package]
name = "{package_name}"
version = "0.1.0"
edition = "2024"

[dependencies]
axum = "0.8"
tokio = {{ version = "1", features = ["macros", "rt-multi-thread", "net"] }}
serde = {{ version = "1", features = ["derive"] }}
serde_json = "1"
tracing = "0.1"
tracing-subscriber = {{ version = "0.3", features = ["env-filter"] }}

[profile.release]
strip = true
"#
    );

    Ok(GeneratedProject {
        package_name,
        main_rs,
        cargo_toml,
    })
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
                )
                .located("<generated>", 1, None::<String>));
            }
            TypeRef::Named(name) => name.clone(),
        };
        Ok(if is_optional {
            format!("Option<{base}>")
        } else {
            base
        })
    }

    // -- Handlers -----------------------------------------------------------

    fn render_handlers(&self, blueprint: &ServiceBlueprint) -> Result<String, Diagnostic> {
        let mut out = String::new();
        for route in &blueprint.routes {
            out.push_str(&self.render_handler(route)?);
            out.push('\n');
        }
        Ok(out)
    }

    fn render_handler(&self, route: &RouteDefinition) -> Result<String, Diagnostic> {
        let params = param_types(route);
        let counts = count_idents_in(route);
        let emitter = Emitter {
            codegen: self,
            params: &params,
            counts: &counts,
        };

        let request = match &route.request {
            RequestSpec::None => String::new(),
            RequestSpec::Json { var, ty } => {
                let rust = self.rust_type(ty, false)?;
                format!("Json({var}): Json<{rust}>, ")
            }
        };

        let (return_ty, body) = match &route.response {
            ResponseSpec::None => (String::new(), String::new()),
            ResponseSpec::Json(TypeRef::Named(name)) => {
                let struct_def = self.find_struct(name)?;
                let value = match route.returns.first() {
                    Some(expr) => emitter.render_named(expr, struct_def)?,
                    None => {
                        return Err(Diagnostic::blocker(
                            "E2002",
                            format!("handler must return a `{name}` value"),
                        )
                        .located("<generated>", 1, None::<String>));
                    }
                };
                (format!("Json<{name}>"), format!("    Json({value})\n"))
            }
            ResponseSpec::Json(_) => {
                // dict and primitives serialize through serde_json::Value.
                let value = match route.returns.first() {
                    Some(expr) => emitter.render_value(expr)?,
                    None => "serde_json::Value::Null".to_string(), // Python: `return` without value
                };
                (
                    "Json<serde_json::Value>".to_string(),
                    format!("    Json({value})\n"),
                )
            }
        };

        let story_comment = if route.stories.is_empty() {
            String::new()
        } else {
            format!(" // stories: {}", route.stories.join(", "))
        };

        let signature = if request.is_empty() {
            format!("async fn {}()", route.handler_name)
        } else {
            format!("async fn {}({request})", route.handler_name)
        };
        let return_clause = if return_ty.is_empty() {
            String::new()
        } else {
            format!(" -> {return_ty}")
        };

        Ok(format!(
            "{signature}{return_clause} {{{story_comment}\n{body}}}\n",
            story_comment = story_comment,
            body = body,
        ))
    }

    fn render_router(&self, blueprint: &ServiceBlueprint) -> String {
        let mut lines = Vec::new();
        for route in &blueprint.routes {
            let router_fn = route.method.axum_router_fn();
            lines.push(format!(
                "        .route({}, {router_fn}({}))",
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
            )
            .located("<generated>", 1, None::<String>)
        })
    }
}

/// Bundled main-file inputs so `assemble` stays under the argument-count
/// threshold.
struct MainParts {
    routing: String,
    host: String,
    port: u16,
}

/// The axum routing functions the blueprint actually uses, in first-use
/// order, so the generated `use` list has no unused imports.
fn used_router_fns(blueprint: &ServiceBlueprint) -> Vec<&'static str> {
    let mut seen: Vec<&'static str> = Vec::new();
    for route in &blueprint.routes {
        let name = route.method.axum_router_fn();
        if !seen.contains(&name) {
            seen.push(name);
        }
    }
    seen
}

/// Assemble the final `main.rs` from its parts.
fn assemble(
    structs: &str,
    handlers: &str,
    router: &str,
    parts: &MainParts,
) -> Result<String, Diagnostic> {
    // The runtime helpers are always emitted and annotated so unused ones do
    // not warn in the generated crate.
    let helpers = "\
#[allow(dead_code)]
fn json_obj(pairs: Vec<(&'static str, serde_json::Value)>) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    for (key, value) in pairs {
        map.insert(key.to_string(), value);
    }
    serde_json::Value::Object(map)
}

#[allow(dead_code)]
fn json_number(value: f64) -> serde_json::Value {
    serde_json::Number::from_f64(value)
        .map(serde_json::Value::Number)
        .expect(\"finite float literal\")
}

";
    Ok(format!(
        r#"// Generated by rivet {version}. Do not edit; run `rivet build` again.
use axum::{{extract::Json, routing::{{{routing}}}, Router}};

{structs}{helpers}{handlers}
#[tokio::main]
async fn main() {{
    tracing_subscriber::fmt::init();

    let app = Router::new()
{router};
    let host = std::env::var("HOST").unwrap_or_else(|_| "{host}".to_string());
    let port: u16 = std::env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or({port});
    let addr = format!("{{host}}:{{port}}");
    let listener = tokio::net::TcpListener::bind(&addr).await.expect("failed to bind address");
    println!("rivet app listening on http://{{addr}}");
    axum::serve(listener, app).await.expect("server error");
}}
"#,
        version = env!("CARGO_PKG_VERSION"),
        routing = parts.routing,
        structs = structs,
        helpers = helpers,
        handlers = handlers,
        router = router,
        host = parts.host,
        port = parts.port,
    ))
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
            )
            .located("<generated>", 1, None::<String>)),
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
                    )
                    .located("<generated>", 1, None::<String>));
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
                )
                .located("<generated>", 1, None::<String>)),
            },
            Expr::Construct { .. } => Err(Diagnostic::blocker(
                "E2002",
                "DTO construction nested inside a JSON value is not supported yet",
            )
            .located("<generated>", 1, None::<String>)),
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
        )
        .located("<generated>", 1, None::<String>)
    }
}

// ---------------------------------------------------------------------------
// Free helpers
// ---------------------------------------------------------------------------

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
fn crate_name(name: &str) -> String {
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
mod tests {
    use super::*;
    use rivet_core::ir::HttpMethod;

    fn ping_blueprint() -> ServiceBlueprint {
        ServiceBlueprint {
            name: "app".to_string(),
            structs: vec![],
            routes: vec![RouteDefinition {
                method: HttpMethod::Get,
                path: "/ping".to_string(),
                handler_name: "ping".to_string(),
                stories: vec![],
                middlewares: vec![],
                request: RequestSpec::None,
                response: ResponseSpec::Json(TypeRef::Json),
                returns: vec![Expr::Object(vec![(
                    "status".to_string(),
                    Expr::Str("pong".to_string()),
                )])],
            }],
            dependencies: vec![],
        }
    }

    #[test]
    fn crate_name_is_sanitized() {
        assert_eq!(crate_name("my-api"), "my-api");
        assert_eq!(crate_name("My Cool API!"), "my-cool-api");
        assert_eq!(crate_name(""), "app");
        assert_eq!(crate_name("9lives"), "app-9lives");
    }

    #[test]
    fn renders_ping_route() {
        let config = RivetConfig::default();
        let project = generate_project(&ping_blueprint(), &config).expect("generate");
        assert!(project.main_rs.contains(".route(\"/ping\", get(ping))"));
        assert!(
            project
                .main_rs
                .contains("async fn ping() -> Json<serde_json::Value>"),
            "ping main_rs:\n{}",
            project.main_rs
        );
        assert!(
            project
                .main_rs
                .contains("json_obj(vec![(\"status\", serde_json::Value::from(\"pong\"))])")
        );
        assert!(project.cargo_toml.contains("name = \"app\""));
    }

    #[test]
    fn renders_dto_response() {
        let blueprint = ServiceBlueprint {
            name: "app".to_string(),
            structs: vec![StructDefinition {
                name: "OrderResponse".to_string(),
                fields: vec![FieldDefinition {
                    name: "status".to_string(),
                    type_ref: TypeRef::String,
                    is_optional: false,
                    is_borrowed: false,
                }],
            }],
            routes: vec![RouteDefinition {
                method: HttpMethod::Post,
                path: "/orders".to_string(),
                handler_name: "create_order".to_string(),
                stories: vec!["US-123".to_string()],
                middlewares: vec![],
                request: RequestSpec::None,
                response: ResponseSpec::Json(TypeRef::Named("OrderResponse".to_string())),
                returns: vec![Expr::Construct {
                    ty: "OrderResponse".to_string(),
                    args: vec![("status".to_string(), Expr::Str("ok".to_string()))],
                }],
            }],
            dependencies: vec![],
        };
        let config = RivetConfig::default();
        let project = generate_project(&blueprint, &config).expect("generate");
        assert!(project.main_rs.contains("pub struct OrderResponse"));
        assert!(
            project
                .main_rs
                .contains("async fn create_order() -> Json<OrderResponse>"),
            "dto main_rs:\n{}",
            project.main_rs
        );
        assert!(project.main_rs.contains("status: \"ok\""));
    }
}
