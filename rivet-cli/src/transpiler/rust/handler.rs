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
//!
//! A route whose request body borrows from its own bytes takes
//! `axum::body::Bytes` rather than `Json`: the `Json` extractor needs
//! `DeserializeOwned`, and a `&'a str` field is the opposite of owned. The
//! handler decodes the bytes itself, so the borrow has a buffer to point
//! into. That type is written as a full path, for the same reason `Json` and
//! `State` are: a local item cannot shadow an explicit path.

use super::Codegen;
use super::service::render_route;
use crate::diagnostic::Diagnostic;
use rivet_core::ir::{RequestSpec, RouteDefinition, RouteParam, ServiceBlueprint, TypeRef};
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

    // The extraction order is a contract: the `Path` extractor first (one
    // value, or a tuple in path order), then `State`, then `Query` when the
    // route reads one, then the body extractor. `super::Path` is spelled out
    // for the same reason `Json` and `State` are: a local item cannot shadow
    // an explicit path.
    let (path_extraction, path_args) = match route.path_params.as_slice() {
        [] => (String::new(), String::new()),
        [param] => {
            let ty = codegen.rust_type(&param.ty, false)?;
            (
                format!("super::Path({}): super::Path<{ty}>", param.name),
                param.name.clone(),
            )
        }
        params => {
            let names: Vec<&str> = params.iter().map(|param| param.name.as_str()).collect();
            let types: Vec<String> = params
                .iter()
                .map(|param| codegen.rust_type(&param.ty, false))
                .collect::<Result<_, _>>()?;
            (
                format!(
                    "super::Path(({})): super::Path<({})>",
                    names.join(", "),
                    types.join(", ")
                ),
                names.join(", "),
            )
        }
    };

    // A query parameter arrives as one raw string in the map, so the handler
    // binds each declared one from it, in declaration order, and answers 400
    // on a missing key or a value that does not parse.
    let query_args = if route.query_params.is_empty() {
        String::new()
    } else {
        route
            .query_params
            .iter()
            .map(|param| param.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut query_binding = String::new();
    for param in &route.query_params {
        query_binding.push_str(&render_query_binding(codegen, param)?);
    }

    // `super::Json` and `super::State` are spelled out so a handler named
    // after either extractor cannot capture its own pattern. `Bytes` is
    // written as its full path for the same reason: it is an opaque struct,
    // so it binds by name and its type is `axum::body::Bytes`.
    let (body_extraction, binding, body_arg) = match &route.request {
        RequestSpec::None => (String::new(), String::new(), String::new()),
        RequestSpec::Json { var, ty } if codegen.borrows(ty) => {
            let rust_type = codegen.rust_type(ty, false)?;
            (
                "bytes: axum::body::Bytes".to_string(),
                format!(
                    "    let {var}: {rust_type} = match serde_json::from_slice(&bytes) {{\n        Ok(value) => value,\n        Err(err) => return Err((axum::http::StatusCode::BAD_REQUEST, format!(\"cannot read the request body: {{err}}\"))),\n    }};\n"
                ),
                var.clone(),
            )
        }
        RequestSpec::Json { var, ty } => (
            format!(
                "super::Json({var}): super::Json<{}>",
                codegen.rust_type(ty, false)?
            ),
            String::new(),
            var.clone(),
        ),
    };

    let mut extraction = Vec::new();
    if !path_extraction.is_empty() {
        extraction.push(path_extraction);
    }
    extraction.push("super::State(channel): super::State<C>".to_string());
    if !route.query_params.is_empty() {
        extraction.push(
            "super::Query(query): super::Query<std::collections::HashMap<String, String>>"
                .to_string(),
        );
    }
    if !body_extraction.is_empty() {
        extraction.push(body_extraction);
    }
    let extraction = extraction.join(", ");

    // The channel call argument list: the path variables, then the query
    // variables, then the body variable, so `search` calls
    // `channel.search(page, size)`.
    let mut args = Vec::new();
    if !path_args.is_empty() {
        args.push(path_args);
    }
    if !query_args.is_empty() {
        args.push(query_args);
    }
    if !body_arg.is_empty() {
        args.push(body_arg);
    }
    let argument = args.join(", ");
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
        "/// Serves `{method} {path}`{stories}.\n///\n/// Calls the service layer over the configured channel.\n#[allow(non_snake_case)]\npub async fn {name}<C: channel::Channel>({extraction}) -> {return_ty} {{\n{query_binding}{binding}    {call}\n}}\n",
        method = route.method.as_str(),
        path = route.path,
        name = route.handler_name,
    ))
}

/// Bind one query parameter from the extracted map, in declaration order.
///
/// A missing key and a value that does not parse both answer 400: the handler
/// declared the parameter and its type, so either way the request is
/// malformed. `str` values are taken as-is; the numeric and boolean types
/// parse the text.
fn render_query_binding(codegen: &Codegen<'_>, param: &RouteParam) -> Result<String, Diagnostic> {
    let name = &param.name;
    let ty = codegen.rust_type(&param.ty, false)?;
    let missing = format!(
        "return Err((axum::http::StatusCode::BAD_REQUEST, \"the query parameter `{name}` is missing\".to_string()))"
    );
    let value = if param.ty == TypeRef::String {
        "value.clone()".to_string()
    } else {
        let label = super::type_label(&param.ty);
        format!(
            "match value.parse() {{\n            Ok(value) => value,\n            Err(_) => return Err((axum::http::StatusCode::BAD_REQUEST, format!(\"the query parameter `{name}` must parse as {label}, got `{{value}}`\"))),\n        }}"
        )
    };
    Ok(format!(
        "    let {name}: {ty} = match query.get(\"{name}\") {{\n        Some(value) => {value},\n        None => {missing},\n    }};\n"
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
