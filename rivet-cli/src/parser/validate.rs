//! Semantic validation of a parsed route plus DTO reachability.
//!
//! The parser lowers syntax; this module checks that a handler's return
//! values fit its declared response type, that DTO constructions name real
//! fields, and that the blueprint carries every DTO a route mentions.

use crate::diagnostic::Diagnostic;
use rivet_core::infer;
use rivet_core::ir::{Expr, ResponseSpec, RouteDefinition, Stmt, StructDefinition, TypeRef};
mod dto;
mod reach;

pub(crate) use reach::{ensure_known_dtos, reachable_structs};

use dto::validate_construct;

use std::collections::HashMap;

/// Check a route's body against its response type and parameter types.
///
/// Every `return` in the body must produce the declared type, and a response
/// that carries a concrete value must return on every path. A `-> dict` or
/// `-> None` handler may fall off its end, because Python returns `None`
/// there and the generator answers null.
pub(crate) fn validate_returns(
    route: &RouteDefinition,
    file: &str,
    dtos: &HashMap<String, StructDefinition>,
    param_types: &HashMap<String, TypeRef>,
) -> Result<(), Diagnostic> {
    // The environment starts at the parameters and gains every local, so a
    // `return` can name a value an earlier statement bound.
    let mut env = param_types.clone();
    check_block(&route.body, route, file, dtos, &mut env)?;

    // A concrete response type must be produced on every path, because the
    // generated function has to return it; `dict` and `None` may be absent.
    let must_return =
        matches!(&route.response, ResponseSpec::Json(target) if target != &TypeRef::Json);
    if must_return && !Stmt::all_paths_return(&route.body) {
        return Err(Diagnostic::blocker(
            "E1010",
            format!(
                "handler must return a value of type `{}` on every path",
                match &route.response {
                    ResponseSpec::Json(target) => target.label(),
                    ResponseSpec::None => "None".to_string(),
                }
            ),
            "add a `return`, or give every branch of an `if` its own `return` and an `else`",
        )
        .located(file, 1));
    }
    Ok(())
}

/// Check every return in one block, in source order.
///
/// `env` gains each local as the walk reaches it, so a `return` below an
/// assignment can name the value that assignment bound.
fn check_block(
    body: &[Stmt],
    route: &RouteDefinition,
    file: &str,
    dtos: &HashMap<String, StructDefinition>,
    env: &mut HashMap<String, TypeRef>,
) -> Result<(), Diagnostic> {
    for statement in body {
        match statement {
            Stmt::Return(value) => check_return(value, route, file, dtos, env)?,
            Stmt::Assign { name, ty, .. } => {
                env.insert(name.clone(), ty.clone());
            }
            Stmt::If {
                branches,
                otherwise,
            } => {
                for (_, branch) in branches {
                    check_block(branch, route, file, dtos, env)?;
                }
                check_block(otherwise, route, file, dtos, env)?;
            }
        }
    }
    Ok(())
}

/// Check one `return` value against the route's declared response type.
fn check_return(
    value: &Expr,
    route: &RouteDefinition,
    file: &str,
    dtos: &HashMap<String, StructDefinition>,
    env: &HashMap<String, TypeRef>,
) -> Result<(), Diagnostic> {
    match &route.response {
        ResponseSpec::None => {
            if matches!(value, Expr::Null) {
                Ok(())
            } else {
                Err(Diagnostic::blocker(
                    "E1010",
                    "handler declares `-> None` but returns a value",
                    "return nothing with a bare `return`, or change the return annotation",
                )
                .located(file, 1))
            }
        }
        ResponseSpec::Json(target) => validate_value(value, target, file, dtos, env),
    }
}
/// Check one return value against its declared type.
fn validate_value(
    value: &Expr,
    target: &TypeRef,
    file: &str,
    dtos: &HashMap<String, StructDefinition>,
    params: &HashMap<String, TypeRef>,
) -> Result<(), Diagnostic> {
    // A computed value carries no shape of its own, so check it by the type
    // inference resolves. The structural checks below then only ever see a
    // literal, a name, or a constructor.
    if matches!(value, Expr::Binary { .. } | Expr::Not(_)) {
        return match infer::infer(value, params) {
            Ok(ty) if &ty == target => Ok(()),
            Ok(ty) => Err(Diagnostic::blocker(
                "E1010",
                format!(
                    "the expression produces `{}`; the handler declares `{}`",
                    ty.label(),
                    target.label()
                ),
                "return a value of the declared type, or change the return annotation",
            )
            .located(file, 1)),
            Err(error) => Err(Diagnostic::blocker(
                "E1010",
                error.message(),
                "make every operand a parameter or a local whose type the front end can resolve",
            )
            .located(file, 1)),
        };
    }
    match target {
        TypeRef::Json => validate_json_value(value, file, params),
        TypeRef::String | TypeRef::Bool | TypeRef::Int | TypeRef::Float => {
            validate_primitive(value, target, file, params)
        }
        TypeRef::Array { .. } => validate_array(value, target, file, params, dtos),
        TypeRef::Named(name) => validate_construct(value, name, file, dtos, params),
    }
}

/// A `dict` response accepts any expression that serializes to JSON:
/// literals, containers, and parameter references. DTO construction inside a
/// `dict` is not supported yet.
fn validate_json_value(
    value: &Expr,
    file: &str,
    params: &HashMap<String, TypeRef>,
) -> Result<(), Diagnostic> {
    match value {
        Expr::Ident(var) => {
            if !params.contains_key(var) {
                return Err(unknown_parameter(var, file));
            }
            Ok(())
        }
        Expr::Construct { .. } => Err(Diagnostic::blocker(
            "E1010",
            "constructing a DTO inside a `dict` response is not supported yet",
            "return the DTO directly, or build the dict from literals and request values",
        )
        .located(file, 1)),
        Expr::Array(items) => items
            .iter()
            .try_for_each(|item| validate_json_value(item, file, params)),
        Expr::Object(entries) => entries
            .iter()
            .try_for_each(|(_, item)| validate_json_value(item, file, params)),
        _ => Ok(()), // literals always serialize
    }
}

/// A primitive response must be a matching literal or a parameter of the same
/// type.
fn validate_primitive(
    value: &Expr,
    target: &TypeRef,
    file: &str,
    params: &HashMap<String, TypeRef>,
) -> Result<(), Diagnostic> {
    match value {
        Expr::Str(_) if *target == TypeRef::String => Ok(()),
        Expr::Int(_) if *target == TypeRef::Int => Ok(()),
        Expr::Int(_) if *target == TypeRef::Float => Ok(()), // Python widens ints to floats
        Expr::Float(_) if *target == TypeRef::Float => Ok(()),
        Expr::Bool(_) if *target == TypeRef::Bool => Ok(()),
        Expr::Ident(var) => match params.get(var) {
            Some(ty) if ty == target => Ok(()),
            Some(_) => Err(Diagnostic::blocker(
                "E1010",
                format!("parameter `{var}` does not match the declared response type"),
                "return a value of the declared type or change the parameter's annotation",
            )
            .located(file, 1)),
            None => Err(unknown_parameter(var, file)),
        },
        _ => Err(Diagnostic::blocker(
            "E1010",
            format!(
                "return value does not match the declared type `{}`",
                target.label()
            ),
            format!(
                "return a literal or request parameter of type `{}`",
                target.label()
            ),
        )
        .located(file, 1)),
    }
}

fn validate_array(
    value: &Expr,
    target: &TypeRef,
    file: &str,
    params: &HashMap<String, TypeRef>,
    dtos: &HashMap<String, StructDefinition>,
) -> Result<(), Diagnostic> {
    let TypeRef::Array { element, .. } = target else {
        return Err(Diagnostic::blocker(
            "E1010",
            "internal: expected an array target",
            "report this internal error with the handler source; it is not a user-code issue",
        )
        .located(file, 1));
    };
    match value {
        Expr::Array(items) => items
            .iter()
            .try_for_each(|item| validate_value(item, element, file, dtos, params)),
        Expr::Ident(var) => match params.get(var) {
            Some(ty) if ty == target => Ok(()),
            Some(_) => Err(Diagnostic::blocker(
                "E1010",
                format!("parameter `{var}` does not match the declared `list` response type"),
                "return a list of the declared element type or change the parameter's annotation",
            )
            .located(file, 1)),
            None => Err(unknown_parameter(var, file)),
        },
        _ => Err(Diagnostic::blocker(
            "E1010",
            "return value does not match the declared `list` response type",
            "return a list literal of the declared element type or a request parameter typed as that list",
        )
        .located(file, 1)),
    }
}

fn unknown_parameter(var: &str, file: &str) -> Diagnostic {
    Diagnostic::blocker(
        "E1010",
        format!("`{var}` is not a parameter of this handler"),
        format!(
            "refer only to the handler's request parameter or a literal; check the spelling of `{var}`"
        ),
    )
    .located(file, 1)
}
