//! `api` decorators in the Python DSL.
//!
//! A route decorator is `@api.<method>(path, stories=[...])`. Decorators from
//! any other module are not routes; the caller skips the function.

use crate::diagnostic::Diagnostic;
use crate::parser::expr;
use crate::parser::{NamedChildren, line_of, node_text};
use rivet_core::ir::HttpMethod;
use tree_sitter::Node;

/// Parse a decorator node. Returns `Ok(None)` when the decorator does not
/// target the `api` object (it is a plain Python decorator).
pub(crate) fn parse_api_decorator(
    decorator: &Node<'_>,
    source: &str,
    file: &str,
) -> Result<Option<(HttpMethod, String, Vec<String>)>, Diagnostic> {
    let Some(call) = decorator.named_children_all().first().copied() else {
        return Ok(None); // bare decorator such as @app
    };
    if call.kind() != "call" {
        return Ok(None);
    }
    let function = call.child_by_field_name("function");
    let Some(function) = function else {
        return Ok(None);
    };
    if function.kind() != "attribute" {
        return Ok(None);
    }
    let object = function.child_by_field_name("object");
    if object.map(|n| node_text(&n, source)) != Some("api") {
        return Ok(None);
    }
    let attribute = function.child_by_field_name("attribute");
    let Some(attribute) = attribute else {
        return Err(Diagnostic::blocker(
            "E1005",
            "malformed api decorator",
            "write the decorator as @api.<method>(\"path\"), for example @api.get(\"/ping\")",
        )
        .located(file, line_of(decorator)));
    };
    let method = match_method(node_text(&attribute, source), decorator, file)?;

    let arguments = call.child_by_field_name("arguments");
    let mut path: Option<String> = None;
    let mut stories = Vec::new();
    if let Some(arguments) = arguments {
        for argument in arguments.named_children_all() {
            let argument_line = line_of(&argument);
            match argument.kind() {
                "string" => {
                    if path.is_some() {
                        return Err(Diagnostic::blocker(
                            "E1005",
                            "too many positional arguments",
                            "pass a single path string as the first argument",
                        )
                        .located(file, argument_line));
                    }
                    let decoded =
                        expr::decode_string(node_text(&argument, source)).map_err(|reason| {
                            Diagnostic::blocker(
                                "E1005",
                                reason,
                                "write the path as a plain string literal, for example @api.get(\"/ping\")",
                            )
                            .located(file, argument_line)
                        })?;
                    path = Some(decoded);
                }
                "keyword_argument" => {
                    let name = argument
                        .child_by_field_name("name")
                        .map(|n| node_text(&n, source));
                    if name != Some("stories") {
                        return Err(Diagnostic::blocker(
                            "E1005",
                            format!("unsupported decorator keyword `{}`", name.unwrap_or("?")),
                            "the only supported keyword is stories=[...]",
                        )
                        .located(file, argument_line));
                    }
                    stories = parse_stories(&argument, source, file)?;
                }
                other => {
                    return Err(Diagnostic::blocker(
                        "E1005",
                        format!("unsupported decorator argument `{other}`"),
                        "the api decorator takes a path string and stories=[...]",
                    )
                    .located(file, argument_line));
                }
            }
        }
    }
    let path = path.ok_or_else(|| {
        Diagnostic::blocker(
            "E1005",
            "the api decorator requires a path, for example @api.get(\"/ping\")",
            "add a path string as the decorator's first argument",
        )
        .located(file, line_of(decorator))
    })?;
    Ok(Some((method, path, stories)))
}

fn match_method(name: &str, decorator: &Node<'_>, file: &str) -> Result<HttpMethod, Diagnostic> {
    let method = match name {
        "get" => HttpMethod::Get,
        "post" => HttpMethod::Post,
        "put" => HttpMethod::Put,
        "delete" => HttpMethod::Delete,
        "patch" => HttpMethod::Patch,
        "options" => HttpMethod::Options,
        "head" => HttpMethod::Head,
        other => {
            return Err(Diagnostic::blocker(
                "E1005",
                format!("`api.{other}` is not an HTTP method"),
                "use one of api.get, api.post, api.put, api.delete, api.patch, api.options, api.head",
            )
            .located(file, line_of(decorator)));
        }
    };
    Ok(method)
}

/// Parse `stories=["US-1", ...]` into a list of story IDs.
fn parse_stories(keyword: &Node<'_>, source: &str, file: &str) -> Result<Vec<String>, Diagnostic> {
    let value = keyword.child_by_field_name("value").ok_or_else(|| {
        Diagnostic::blocker(
            "E1005",
            "stories requires a list value",
            "give stories a list of story IDs, for example stories=[\"US-123\"]",
        )
        .located(file, line_of(keyword))
    })?;
    if value.kind() != "list" {
        return Err(Diagnostic::blocker(
            "E1005",
            "stories must be a list of story IDs, for example stories=[\"US-123\"]",
            "replace the value with a list of quoted story IDs, for example stories=[\"US-1\", \"US-2\"]",
        )
        .located(file, line_of(keyword)));
    }
    let mut stories = Vec::new();
    for element in value.named_children_all() {
        if element.kind() != "string" {
            return Err(Diagnostic::blocker(
                "E1005",
                "story IDs must be string literals",
                "write each story ID as a quoted string, for example \"US-123\"",
            )
            .located(file, line_of(&element)));
        }
        let story = expr::decode_string(node_text(&element, source)).map_err(|reason| {
            Diagnostic::blocker(
                "E1005",
                reason,
                "write the story ID as a plain string literal, for example \"US-123\"",
            )
            .located(file, line_of(&element))
        })?;
        stories.push(story);
    }
    Ok(stories)
}

/// Route paths must start with `/` and, until path-parameter support lands,
/// must not contain placeholders.
pub(crate) fn validate_path(path: &str, file: &str, line: usize) -> Result<(), Diagnostic> {
    if !path.starts_with('/') {
        return Err(Diagnostic::blocker(
            "E1005",
            format!("route path `{path}` must start with `/`"),
            "prefix the route path with `/`, for example \"/ping\"",
        )
        .located(file, line));
    }
    if path.contains('{') || path.contains('}') {
        return Err(Diagnostic::blocker(
            "E1004",
            format!("route path `{path}` uses path parameters, which are not supported yet"),
            "move the parameter into the JSON body for now",
        )
        .located(file, line));
    }
    Ok(())
}
