//! Handler signatures: parameters and return annotations.

use crate::diagnostic::Diagnostic;
use crate::parser::annotation::{parse_type_text, type_label};
use crate::parser::{NamedChildren, is_safe_identifier, line_of, node_text};
use rivet_core::ir::{PathParam, RequestSpec, ResponseSpec, TypeRef};
use std::collections::HashMap;
use tree_sitter::Node;

/// The parsed pieces of a handler signature.
pub(crate) struct ParsedParameters {
    /// The `{name}` path parameters, in path order.
    pub(crate) path_params: Vec<PathParam>,
    /// The request body, when the handler declares one.
    pub(crate) request: RequestSpec,
    /// Every parameter's declared type, for return validation.
    pub(crate) types: HashMap<String, TypeRef>,
}

/// Handler parameters.
///
/// A parameter that names a `{name}` placeholder in the route path becomes a
/// path parameter, in path order. The rest are the request body: at most one,
/// of any type the JSON bridge supports.
///
/// Returns the path parameters, the request spec, and the parameter types.
pub(crate) fn parse_parameters(
    function: &Node<'_>,
    source: &str,
    file: &str,
    placeholders: &[String],
) -> Result<ParsedParameters, Diagnostic> {
    let parameters = function.child_by_field_name("parameters").ok_or_else(|| {
        Diagnostic::blocker(
            "E1005",
            "function without a parameter list",
            "add a parameter list to the function definition, for example `def ping():`",
        )
        .located(file, line_of(function))
    })?;

    let mut params: Vec<(String, TypeRef)> = Vec::new();
    for parameter in parameters.named_children_all() {
        let parameter_line = line_of(&parameter);
        match parameter.kind() {
            "typed_parameter" => {
                // The parameter name is the bare identifier child; `type:` is
                // the only named field in the grammar.
                let name_node = parameter
                    .child_by_field_name("name")
                    .or_else(|| {
                        parameter
                            .named_children_all()
                            .into_iter()
                            .find(|c| c.kind() == "identifier")
                    })
                    .ok_or_else(|| {
                        Diagnostic::blocker(
                            "E1001",
                            "parameter is missing a type hint",
                            "add a type annotation such as `request: dict`",
                        )
                        .located(file, parameter_line)
                    })?;
                let name = node_text(&name_node, source);
                let ty = parameter.child_by_field_name("type").ok_or_else(|| {
                    Diagnostic::blocker(
                        "E1001",
                        "parameter is missing a type hint",
                        "add a type annotation such as `request: dict`",
                    )
                    .located(file, parameter_line)
                })?;
                if !is_safe_identifier(name) {
                    return Err(Diagnostic::blocker(
                        "E1011",
                        format!("parameter name `{name}` is not a safe Rust identifier"),
                        "rename the parameter to a snake_case Rust-safe identifier, for example `request_id`",
                    )
                    .located(file, parameter_line));
                }
                let parsed = parse_type_text(node_text(&ty, source), file, parameter_line, false)?;
                if parsed.is_borrowed {
                    return Err(Diagnostic::blocker(
                        "E1014",
                        format!("request parameter `{name}` cannot borrow from the request body"),
                        "declare a DTO that holds the borrowed field, then take that class as the parameter, for example `request: Note`",
                    )
                    .located(file, parameter_line));
                }
                if parsed.is_optional {
                    return Err(Diagnostic::blocker(
                        "E1004",
                        format!("optional request parameter `{name}` is not supported; a missing body cannot deserialize"),
                        "declare the parameter without Optional[...] or `| None`; a JSON body is always present",
                    )
                    .located(file, parameter_line));
                }
                if params.iter().any(|(existing, _)| existing == name) {
                    return Err(Diagnostic::blocker(
                        "E1004",
                        format!("parameter `{name}` is declared twice"),
                        "give each parameter a distinct name",
                    )
                    .located(file, parameter_line));
                }
                params.push((name.to_string(), parsed.type_ref));
            }
            "identifier" => {
                let name = node_text(&parameter, source);
                return Err(Diagnostic::blocker(
                    "E1001",
                    format!("parameter `{name}` is missing a type hint"),
                    "add a type annotation such as `request: dict`",
                )
                .located(file, parameter_line));
            }
            other => {
                return Err(Diagnostic::blocker(
                    "E1004",
                    format!("`{other}` parameters are not supported (defaults, splats, and keyword-only markers)"),
                    "remove defaults, splats, and markers; declare the request as one plain typed parameter",
                )
                .located(file, parameter_line));
            }
        }
    }

    let mut path_params = Vec::with_capacity(placeholders.len());
    for placeholder in placeholders {
        let Some((_, ty)) = params.iter().find(|(name, _)| name == placeholder) else {
            return Err(Diagnostic::blocker(
                "E1015",
                format!(
                    "route path declares the placeholder `{{{placeholder}}}`, but the handler has no parameter with that name"
                ),
                format!(
                    "add `{placeholder}: int` to the handler signature, or remove `{{{placeholder}}}` from the route path"
                ),
            )
            .located(file, line_of(function)));
        };
        if !matches!(ty, TypeRef::String | TypeRef::Int) {
            return Err(Diagnostic::blocker(
                "E1015",
                format!(
                    "path parameter `{placeholder}` has type `{}`; a path parameter supports `str` and `int`",
                    type_label(ty)
                ),
                "declare the parameter as `str` or `int`, or move the value into the request body",
            )
            .located(file, line_of(function)));
        }
        path_params.push(PathParam {
            name: placeholder.clone(),
            ty: ty.clone(),
        });
    }

    let mut body: Vec<(String, TypeRef)> = params
        .into_iter()
        .filter(|(name, _)| !placeholders.contains(name))
        .collect();
    if body.len() > 1 {
        return Err(Diagnostic::blocker(
            "E1004",
            format!(
                "handler `{}` declares {} parameters that are not path parameters",
                handler_display_name(function, source),
                body.len()
            ),
            "a handler takes path parameters plus at most one request-body parameter; merge the rest into one dict or DTO body",
        )
        .located(file, line_of(function)));
    }

    let mut types = HashMap::new();
    for path_param in &path_params {
        types.insert(path_param.name.clone(), path_param.ty.clone());
    }
    let spec = match body.pop() {
        Some((var, ty)) => {
            types.insert(var.clone(), ty.clone());
            RequestSpec::Json { var, ty }
        }
        None => RequestSpec::None,
    };
    Ok(ParsedParameters {
        path_params,
        request: spec,
        types,
    })
}

/// The return annotation decides the response spec.
pub(crate) fn parse_return_type(
    function: &Node<'_>,
    source: &str,
    file: &str,
) -> Result<ResponseSpec, Diagnostic> {
    let return_type = function.child_by_field_name("return_type");
    let Some(return_type) = return_type else {
        let name = handler_display_name(function, source);
        return Err(Diagnostic::blocker(
            "E1001",
            format!("handler `{name}` is missing a return type hint"),
            "add a return annotation such as `-> dict` or `-> None`",
        )
        .located(file, line_of(function)));
    };
    let annotation = node_text(&return_type, source).trim();
    if annotation == "None" {
        return Ok(ResponseSpec::None);
    }
    let parsed = parse_type_text(annotation, file, line_of(&return_type), false)?;
    if parsed.is_borrowed {
        return Err(Diagnostic::blocker(
            "E1014",
            "a route cannot return a borrowed value",
            "return a DTO whose text fields are owned `str` values",
        )
        .located(file, line_of(&return_type)));
    }
    if parsed.is_optional {
        return Err(Diagnostic::blocker(
            "E1004",
            "an optional response body is not supported; return the plain type",
            "declare the return annotation without Optional[...] or `| None`",
        )
        .located(file, line_of(&return_type)));
    }
    Ok(ResponseSpec::Json(parsed.type_ref))
}

pub(crate) fn handler_display_name(function: &Node<'_>, source: &str) -> String {
    function
        .child_by_field_name("name")
        .map(|n| node_text(&n, source))
        .unwrap_or("<handler>")
        .to_string()
}
