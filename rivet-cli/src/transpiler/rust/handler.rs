//! The axum layer: one thin handler per route over the channel.
//!
//! A handler extracts the request, calls the same channel method the gRPC
//! transport implements, and maps a channel failure to `502`. It holds no
//! route logic, so the configured topology never changes a route's behavior.

use super::Codegen;
use super::service::render_route;
use crate::diagnostic::Diagnostic;
use rivet_core::ir::{RequestSpec, RouteDefinition, ServiceBlueprint};

/// Render one axum handler per route, in blueprint order.
pub(super) fn render_handlers(
    codegen: &Codegen<'_>,
    blueprint: &ServiceBlueprint,
) -> Result<String, Diagnostic> {
    let mut out = String::new();
    for route in &blueprint.routes {
        out.push_str(&render_handler(codegen, route)?);
        out.push('\n');
    }
    Ok(out)
}

/// Render one handler: extract the request, call the channel, map the result.
fn render_handler(codegen: &Codegen<'_>, route: &RouteDefinition) -> Result<String, Diagnostic> {
    let rendered = render_route(codegen, route)?;

    let (extraction, argument) = match &route.request {
        RequestSpec::None => ("State(channel): State<C>".to_string(), String::new()),
        RequestSpec::Json { var, ty } => (
            format!(
                "State(channel): State<C>, Json({var}): Json<{}>",
                codegen.rust_type(ty, false)?
            ),
            var.clone(),
        ),
    };
    let (return_ty, call) = if rendered.return_ty.is_empty() {
        (
            "Result<axum::http::StatusCode, (axum::http::StatusCode, String)>".to_string(),
            format!(
                "channel.{name}({argument})\n        .await\n        .map(|()| axum::http::StatusCode::OK)\n        .map_err(channel_error)",
                name = route.handler_name,
            ),
        )
    } else {
        (
            format!(
                "Result<Json<{}>, (axum::http::StatusCode, String)>",
                rendered.return_ty
            ),
            format!(
                "channel.{name}({argument})\n        .await\n        .map(Json)\n        .map_err(channel_error)",
                name = route.handler_name,
            ),
        )
    };

    let stories = if route.stories.is_empty() {
        String::new()
    } else {
        format!(" — stories: {}", route.stories.join(", "))
    };
    Ok(format!(
        "/// Serves `{method} {path}`{stories}.\n///\n/// Calls the service layer over the configured channel.\nasync fn {name}<C: channel::Channel>({extraction}) -> {return_ty} {{\n    {call}\n}}\n",
        method = route.method.as_str(),
        path = route.path,
        name = route.handler_name,
    ))
}
