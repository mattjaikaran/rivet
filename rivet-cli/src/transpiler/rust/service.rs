//! The transport-free service layer.
//!
//! Every route's logic becomes one async function over plain Rust types: no
//! axum extractor and no response wrapper. The generated channel calls these
//! functions, either directly or over gRPC, so the configured topology never
//! changes a route's code.

use super::{Codegen, Emitter, count_idents_in, param_types};
use crate::diagnostic::Diagnostic;
use rivet_core::ir::{RequestSpec, ResponseSpec, RouteDefinition, ServiceBlueprint, TypeRef};
use rivet_core::reserved;

/// Render `mod service`: one function per route, in blueprint order.
pub(super) fn render_service_module(
    codegen: &Codegen<'_>,
    blueprint: &ServiceBlueprint,
) -> Result<String, Diagnostic> {
    let mut body = String::new();
    for route in &blueprint.routes {
        body.push_str(&render_fn(codegen, route)?);
    }
    let mut out = format!(
        "/// Transport-free route logic. The channel calls these functions.\nmod {} {{\n    use super::*;\n\n",
        reserved::SERVICE_MODULE
    );
    out.push_str(body.trim_end());
    out.push_str("\n}\n");
    Ok(out)
}

/// Render one route as a service function.
///
/// The function carries the route logic and the route's source comment, so a
/// reader of the generated crate sees which DSL route produced it.
fn render_fn(codegen: &Codegen<'_>, route: &RouteDefinition) -> Result<String, Diagnostic> {
    let rendered = render_route(codegen, route)?;

    let signature = if rendered.params.is_empty() {
        format!("async fn {}()", route.handler_name)
    } else {
        format!("async fn {}({})", route.handler_name, rendered.params)
    };
    let return_clause = if rendered.return_ty.is_empty() {
        String::new()
    } else {
        format!(" -> {}", rendered.return_ty)
    };
    let body = if rendered.body.is_empty() {
        String::new()
    } else {
        format!("\n        {}", rendered.body)
    };
    // The function carries a user-chosen name, so it takes
    // `#[allow(non_snake_case)]`: the DSL decides the name, and a cargo style
    // lint against generated code is noise the user cannot act on. The front
    // end owns identifier policy.
    Ok(format!(
        "    // {method} {path}\n    #[allow(non_snake_case)]\n    pub {signature}{return_clause} {{{body}\n    }}\n\n",
        method = route.method.as_str(),
        path = route.path,
    ))
}

/// The pieces of one rendered route: parameters, return type, and body.
pub(super) struct RenderedRoute {
    /// The parameter list without an extractor, or empty.
    pub(super) params: String,
    /// The Rust return type, or empty for a route that returns nothing.
    pub(super) return_ty: String,
    /// The body expression that produces the return value, or empty.
    pub(super) body: String,
}

/// Render a route's parameters, return type, and body once, so the service
/// function and the channel agree on the shape.
pub(super) fn render_route(
    codegen: &Codegen<'_>,
    route: &RouteDefinition,
) -> Result<RenderedRoute, Diagnostic> {
    let params = param_types(route);
    let counts = count_idents_in(route);
    let emitter = Emitter {
        codegen,
        params: &params,
        counts: &counts,
    };

    let rendered_params = match &route.request {
        RequestSpec::None => String::new(),
        RequestSpec::Json { var, ty } => format!("{var}: {}", codegen.rust_type(ty, false)?),
    };

    let (return_ty, body) = match &route.response {
        ResponseSpec::None => (String::new(), String::new()),
        ResponseSpec::Json(TypeRef::Named(name)) => {
            let struct_def = codegen.find_struct(name)?;
            let value = match route.returns.first() {
                Some(expr) => emitter.render_named(expr, struct_def)?,
                None => {
                    return Err(Diagnostic::blocker(
                        "E2002",
                        format!("handler must return a `{name}` value"),
                        format!("make the handler `return` a `{name}` construction, or return a request parameter of type `{name}`"),
                    )
                    .located("<generated>", 1));
                }
            };
            (name.clone(), value)
        }
        // `dict` and the primitives serialize through serde_json::Value.
        ResponseSpec::Json(_) => {
            let value = match route.returns.first() {
                Some(expr) => emitter.render_value(expr)?,
                // Python: a bare `return` with no value.
                None => "serde_json::Value::Null".to_string(),
            };
            ("serde_json::Value".to_string(), value)
        }
    };

    Ok(RenderedRoute {
        params: rendered_params,
        return_ty,
        body,
    })
}
