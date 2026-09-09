//! Handler signatures: parameters and return annotations.

use crate::diagnostic::Diagnostic;
use crate::parser::types::parse_type_text;
use crate::parser::{NamedChildren, is_safe_identifier, line_of, node_text};
use rivet_core::ir::{RequestSpec, ResponseSpec, TypeRef};
use std::collections::HashMap;
use tree_sitter::Node;

/// Handler parameters. Phase 0 permits zero parameters or one JSON-body
/// parameter. Returns the request spec and a parameter-name-to-type map used
/// for return validation.
pub(crate) fn parse_parameters(
    function: &Node<'_>,
    source: &str,
    file: &str,
) -> Result<(RequestSpec, HashMap<String, TypeRef>), Diagnostic> {
    let parameters = function.child_by_field_name("parameters").ok_or_else(|| {
        Diagnostic::blocker("E1005", "function without a parameter list").located(
            file,
            line_of(function),
            None::<String>,
        )
    })?;

    let mut params: Vec<(String, TypeRef)> = Vec::new();
    for parameter in parameters.named_children_all() {
        let parameter_line = line_of(&parameter);
        match parameter.kind() {
            "typed_parameter" => {
                // The parameter name is the bare identifier child; `type:` is
                // the only named field in the grammar.
                let name_node =
                    parameter
                        .child_by_field_name("name")
                        .or_else(|| {
                            parameter
                                .named_children_all()
                                .into_iter()
                                .find(|c| c.kind() == "identifier")
                        })
                        .ok_or_else(|| {
                            Diagnostic::blocker("E1001", "parameter is missing a type hint")
                                .located(file, parameter_line, None::<String>)
                        })?;
                let name = node_text(&name_node, source);
                let ty = parameter.child_by_field_name("type").ok_or_else(|| {
                    Diagnostic::blocker("E1001", "parameter is missing a type hint").located(
                        file,
                        parameter_line,
                        Some("add a type annotation such as `request: dict`"),
                    )
                })?;
                if !is_safe_identifier(name) {
                    return Err(Diagnostic::blocker(
                        "E1011",
                        format!("parameter name `{name}` is not a safe Rust identifier"),
                    )
                    .located(file, parameter_line, None::<String>));
                }
                let (type_ref, is_optional) =
                    parse_type_text(node_text(&ty, source), file, parameter_line, false)?;
                if is_optional {
                    return Err(Diagnostic::blocker(
                        "E1004",
                        format!("optional request parameter `{name}` is not supported; a missing body cannot deserialize"),
                    )
                    .located(file, parameter_line, None::<String>));
                }
                params.push((name.to_string(), type_ref));
            }
            "identifier" => {
                let name = node_text(&parameter, source);
                return Err(Diagnostic::blocker(
                    "E1001",
                    format!("parameter `{name}` is missing a type hint"),
                )
                .located(
                    file,
                    parameter_line,
                    Some("add a type annotation such as `request: dict`"),
                ));
            }
            other => {
                return Err(Diagnostic::blocker(
                    "E1004",
                    format!("`{other}` parameters are not supported (defaults, splats, and keyword-only markers)"),
                )
                .located(file, parameter_line, None::<String>));
            }
        }
    }

    if params.len() > 1 {
        return Err(Diagnostic::blocker(
            "E1004",
            format!(
                "handler `{}` declares {} parameters; phase 0 supports a single request-body parameter",
                handler_display_name(function, source),
                params.len()
            ),
        )
        .located(
            file,
            line_of(function),
            Some("merge the parameters into one dict or DTO body"),
        ));
    }

    let mut types = HashMap::new();
    let spec = match params.pop() {
        Some((var, ty)) => {
            types.insert(var.clone(), ty.clone());
            RequestSpec::Json { var, ty }
        }
        None => RequestSpec::None,
    };
    Ok((spec, types))
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
        )
        .located(
            file,
            line_of(function),
            Some("add a return annotation such as `-> dict` or `-> None`"),
        ));
    };
    let annotation = node_text(&return_type, source).trim();
    if annotation == "None" {
        return Ok(ResponseSpec::None);
    }
    let (type_ref, is_optional) = parse_type_text(annotation, file, line_of(&return_type), false)?;
    if is_optional {
        return Err(Diagnostic::blocker(
            "E1004",
            "an optional response body is not supported; return the plain type",
        )
        .located(file, line_of(&return_type), None::<String>));
    }
    Ok(ResponseSpec::Json(type_ref))
}

pub(crate) fn handler_display_name(function: &Node<'_>, source: &str) -> String {
    function
        .child_by_field_name("name")
        .map(|n| node_text(&n, source))
        .unwrap_or("<handler>")
        .to_string()
}
