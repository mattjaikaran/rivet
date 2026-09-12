//! The transport-free service layer.
//!
//! Every route's logic becomes one async function over plain Rust types: no
//! axum extractor and no response wrapper. The generated channel calls these
//! functions, either directly or over gRPC, so the configured topology never
//! changes a route's code.

use super::body::{BodyContext, render_body};
use super::{Codegen, Emitter, count_idents_in, param_types};
use crate::diagnostic::Diagnostic;
use rivet_core::ir::{RequestSpec, ResponseSpec, RouteDefinition, ServiceBlueprint, Stmt, TypeRef};
use rivet_core::reserved;
use std::collections::HashSet;

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

    let signature = if rendered.params().is_empty() {
        format!("async fn {}()", route.handler_name)
    } else {
        format!("async fn {}({})", route.handler_name, rendered.params())
    };
    let return_clause = if rendered.return_ty.is_empty() {
        String::new()
    } else {
        format!(" -> {}", rendered.return_ty)
    };
    let body = if rendered.body.is_empty() {
        String::new()
    } else {
        // A statement block spans several lines; indent each one, not just
        // the first, so the block sits inside the function.
        format!("\n{}", indented(&rendered.body, 8))
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
    /// Path parameter `(name, Rust type)` pairs, in path order.
    pub(super) path_params: Vec<(String, String)>,
    /// Query parameter `(name, Rust type)` pairs, in declaration order.
    pub(super) query_params: Vec<(String, String)>,
    /// The JSON-body parameter `(name, Rust type)`, when the route takes one.
    pub(super) request: Option<(String, String)>,
    /// The Rust return type, or empty for a route that returns nothing.
    pub(super) return_ty: String,
    /// The rendered statement block, or empty for a bodyless route.
    pub(super) body: String,
}

impl RenderedRoute {
    /// The full parameter list, without a leading separator: `id: i64, request: Order`.
    pub(super) fn params(&self) -> String {
        let mut params: Vec<String> = self
            .path_params
            .iter()
            .chain(self.query_params.iter())
            .map(|(name, ty)| format!("{name}: {ty}"))
            .collect();
        if let Some((name, ty)) = &self.request {
            params.push(format!("{name}: {ty}"));
        }
        params.join(", ")
    }
}

/// Render a route's parameters, return type, and body once, so the service
/// function and the channel agree on the shape.
pub(super) fn render_route(
    codegen: &Codegen<'_>,
    route: &RouteDefinition,
) -> Result<RenderedRoute, Diagnostic> {
    let counts = count_idents_in(route);
    let mut env = param_types(route);
    // Names this renderer emitted a `let` for, so a later assignment to one
    // becomes a plain reassignment instead of a second `let`.
    let mut declared = HashSet::new();

    let path_params: Vec<(String, String)> = route
        .path_params
        .iter()
        .map(|param| Ok((param.name.clone(), codegen.rust_type(&param.ty, false)?)))
        .collect::<Result<_, Diagnostic>>()?;

    let query_params: Vec<(String, String)> = route
        .query_params
        .iter()
        .map(|param| Ok((param.name.clone(), codegen.rust_type(&param.ty, false)?)))
        .collect::<Result<_, Diagnostic>>()?;

    let request = match &route.request {
        RequestSpec::None => None,
        RequestSpec::Json { var, ty } => Some((var.clone(), codegen.rust_type(ty, false)?)),
    };

    let return_ty = match &route.response {
        ResponseSpec::None => String::new(),
        ResponseSpec::Json(TypeRef::Named(name)) => name.clone(),
        ResponseSpec::Json(_) => "serde_json::Value".to_string(),
    };

    let mut body = match route.body.as_slice() {
        [] if matches!(route.response, ResponseSpec::None) => String::new(),
        [Stmt::Return(expr)] if !matches!(route.response, ResponseSpec::None) => {
            // A single return is the common shape; render it as a tail
            // expression so the generated function stays byte-for-byte the
            // same as before the body gained statements.
            let emitter = Emitter {
                codegen,
                params: &env,
                counts: &counts,
            };
            emitter.render_return_value(expr, &route.response)?
        }
        _ => render_body(
            &BodyContext {
                codegen,
                counts: &counts,
                response: &route.response,
            },
            route.body.as_slice(),
            &mut env,
            &mut declared,
            0,
        )?,
    };

    // A `dict` route may fall off its end: Python answers `None` and the route
    // answers JSON null, so the generated function needs that tail. Every
    // other statement leaves `()` behind, because a Rust block whose last
    // statement is a `for` or a branch evaluates to `()` rather than diverging.
    if matches!(route.response, ResponseSpec::Json(TypeRef::Json))
        && !Stmt::all_paths_return(&route.body)
    {
        body.push_str("serde_json::Value::Null\n");
    }

    Ok(RenderedRoute {
        path_params,
        query_params,
        request,
        return_ty,
        body,
    })
}

/// Shift every line of `text` right by `spaces`.
fn indented(text: &str, spaces: usize) -> String {
    let pad = " ".repeat(spaces);
    text.lines()
        .map(|line| format!("{pad}{line}"))
        .collect::<Vec<_>>()
        .join("\n")
}
