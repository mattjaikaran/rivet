//! The axum layer: one thin handler per route over the channel.
//!
//! A handler extracts the request, calls the same channel method the gRPC
//! transport implements, and maps a channel failure to `502`. It holds no
//! route logic, so the configured topology never changes a route's behavior.
//!
//! The handlers live inside [`reserved::HANDLERS_MODULE`], not at crate root,
//! and the router reaches them as `handlers::ping::<channel::InProcess>`. The
//! module is what keeps a user's handler name out of the crate root: a
//! handler is a local function, and a local function shadows a glob import
//! without error, so `def service()` compiles here.
//!
//! Two names inside the module still come from the parent, and a user handler
//! can shadow both:
//!
//! - the extractor types `Json` and `State`, whose names the handler takes
//!   from `use super::*`;
//! - the crate-root helper that maps a channel failure.
//!
//! A handler named `State` would rebind the `State(channel)` pattern to the
//! generated function and the crate would not compile. Every such reference
//! is therefore written `super::…`, which a local item cannot shadow — a
//! local item outranks a glob import, but not an explicit path.

use super::Codegen;
use super::service::render_route;
use crate::diagnostic::Diagnostic;
use rivet_core::ir::{RequestSpec, RouteDefinition, ServiceBlueprint};
use rivet_core::reserved;

/// Render one axum handler per route, in blueprint order, inside
/// [`reserved::HANDLERS_MODULE`].
pub(super) fn render_handlers(
    codegen: &Codegen<'_>,
    blueprint: &ServiceBlueprint,
) -> Result<String, Diagnostic> {
    if blueprint.routes.is_empty() {
        return Ok(String::new());
    }
    let mut body = String::new();
    for route in &blueprint.routes {
        body.push_str(&render_handler(codegen, route)?);
        body.push('\n');
    }
    Ok(format!(
        "/// The axum layer: one handler per route. The router calls these.\nmod {} {{\n    use super::*;\n\n{}\n}}\n",
        reserved::HANDLERS_MODULE,
        indented(body.trim_end(), 4)
    ))
}

/// Render one handler: extract the request, call the channel, map the result.
fn render_handler(codegen: &Codegen<'_>, route: &RouteDefinition) -> Result<String, Diagnostic> {
    let rendered = render_route(codegen, route)?;

    // `super::Json` and `super::State` are spelled out so a handler named
    // after either extractor cannot capture its own pattern.
    let (extraction, argument) = match &route.request {
        RequestSpec::None => (
            "super::State(channel): super::State<C>".to_string(),
            String::new(),
        ),
        RequestSpec::Json { var, ty } => (
            format!(
                "super::State(channel): super::State<C>, super::Json({var}): super::Json<{}>",
                codegen.rust_type(ty, false)?
            ),
            var.clone(),
        ),
    };
    let channel_error = format!("{}channel_error", reserved::PREFIX);
    let (return_ty, call) = if rendered.return_ty.is_empty() {
        (
            "Result<axum::http::StatusCode, (axum::http::StatusCode, String)>".to_string(),
            format!(
                "channel.{name}({argument})\n        .await\n        .map(|()| axum::http::StatusCode::OK)\n        .map_err({channel_error})",
                name = route.handler_name,
            ),
        )
    } else {
        (
            format!(
                "Result<super::Json<{}>, (axum::http::StatusCode, String)>",
                rendered.return_ty
            ),
            format!(
                "channel.{name}({argument})\n        .await\n        .map(super::Json)\n        .map_err({channel_error})",
                name = route.handler_name,
            ),
        )
    };

    let stories = if route.stories.is_empty() {
        String::new()
    } else {
        format!(" — stories: {}", route.stories.join(", "))
    };
    // `pub`, because the router at crate root registers the handler. The
    // function carries a user-chosen name, so it takes
    // `#[allow(non_snake_case)]`: the DSL decides the name, and a cargo style
    // lint against generated code is noise the user cannot act on. The front
    // end owns identifier policy.
    Ok(format!(
        "/// Serves `{method} {path}`{stories}.\n///\n/// Calls the service layer over the configured channel.\n#[allow(non_snake_case)]\npub async fn {name}<C: channel::Channel>({extraction}) -> {return_ty} {{\n    {call}\n}}\n",
        method = route.method.as_str(),
        path = route.path,
        name = route.handler_name,
    ))
}

/// Shift every non-empty line of `text` right by `spaces`.
///
/// The rendered handler is written at column zero, where it reads as
/// standalone Rust, and the module wrapper moves the whole block in one step.
fn indented(text: &str, spaces: usize) -> String {
    let pad = " ".repeat(spaces);
    text.lines()
        .map(|line| {
            if line.is_empty() {
                String::new()
            } else {
                format!("{pad}{line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}
