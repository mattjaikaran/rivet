//! Semantic validation of a parsed route plus DTO reachability.
//!
//! The parser lowers syntax; this module checks that a handler's return
//! values fit its declared response type, that DTO constructions name real
//! fields, and that the blueprint carries every DTO a route mentions.

use crate::diagnostic::Diagnostic;
use crate::parser::types::type_label;
use rivet_core::ir::{Expr, RequestSpec, ResponseSpec, RouteDefinition, StructDefinition, TypeRef};
use std::collections::HashMap;

/// Check a route's returns against its response type and parameter types.
pub(crate) fn validate_returns(
    route: &RouteDefinition,
    file: &str,
    dtos: &HashMap<String, StructDefinition>,
    param_types: &HashMap<String, TypeRef>,
) -> Result<(), Diagnostic> {
    match &route.response {
        ResponseSpec::None => {
            for value in &route.returns {
                if !matches!(value, Expr::Null) {
                    return Err(Diagnostic::blocker(
                        "E1010",
                        "handler declares `-> None` but returns a value",
                    )
                    .located(
                        file,
                        1,
                        Some("return nothing, or change the return annotation"),
                    ));
                }
            }
            Ok(())
        }
        ResponseSpec::Json(target) => match route.returns.as_slice() {
            [] => match target {
                TypeRef::Json => Ok(()), // Python returns None; render as JSON null
                _ => Err(Diagnostic::blocker(
                    "E1010",
                    format!(
                        "handler must return a value of type `{}`",
                        type_label(target)
                    ),
                )
                .located(file, 1, Some("add a return statement to the handler"))),
            },
            [value] => validate_value(value, target, file, dtos, param_types),
            _ => Err(Diagnostic::blocker(
                "E1006",
                "multiple return statements are not supported yet",
            )
            .located(file, 1, None::<String>)),
        },
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
        )
        .located(
            file,
            1,
            Some("return the DTO directly, or build the dict from literals and request values"),
        )),
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
            )
            .located(file, 1, None::<String>)),
            None => Err(unknown_parameter(var, file)),
        },
        _ => Err(Diagnostic::blocker(
            "E1010",
            format!(
                "return value does not match the declared type `{}`",
                type_label(target)
            ),
        )
        .located(file, 1, None::<String>)),
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
        return Err(
            Diagnostic::blocker("E1010", "internal: expected an array target").located(
                file,
                1,
                None::<String>,
            ),
        );
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
            )
            .located(file, 1, None::<String>)),
            None => Err(unknown_parameter(var, file)),
        },
        _ => Err(Diagnostic::blocker(
            "E1010",
            "return value does not match the declared `list` response type",
        )
        .located(file, 1, None::<String>)),
    }
}

/// A DTO response must construct that DTO or return a parameter of that type.
fn validate_construct(
    value: &Expr,
    name: &str,
    file: &str,
    dtos: &HashMap<String, StructDefinition>,
    params: &HashMap<String, TypeRef>,
) -> Result<(), Diagnostic> {
    match value {
        Expr::Construct { ty, args } => {
            let struct_def = dtos.get(name).ok_or_else(|| {
                Diagnostic::blocker(
                    "E1003",
                    format!("response type `{name}` is not a defined DTO class"),
                )
                .located(file, 1, None::<String>)
            })?;
            if ty != name {
                return Err(Diagnostic::blocker(
                    "E1010",
                    format!("handler must return `{name}`, not `{ty}`"),
                )
                .located(file, 1, None::<String>));
            }
            let field_types: HashMap<&str, &TypeRef> = struct_def
                .fields
                .iter()
                .map(|f| (f.name.as_str(), &f.type_ref))
                .collect();
            let mut provided: Vec<&str> = Vec::new();
            for (arg_name, arg_value) in args {
                provided.push(arg_name.as_str());
                let field = struct_def.fields.iter().find(|f| f.name == *arg_name);
                let Some(field) = field else {
                    return Err(Diagnostic::blocker(
                        "E1010",
                        format!("`{name}` has no field `{arg_name}`"),
                    )
                    .located(
                        file,
                        1,
                        Some(format!(
                            "available fields: {}",
                            struct_def
                                .fields
                                .iter()
                                .map(|f| f.name.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        )),
                    ));
                };
                let field_ty = field_types[arg_name.as_str()];
                let ok = if matches!(arg_value, Expr::Null) {
                    field.is_optional || field_ty == &TypeRef::Json
                } else {
                    arg_value_matches(arg_value, field_ty, file, params)?
                };
                if !ok {
                    return Err(Diagnostic::blocker(
                        "E1010",
                        format!("field `{arg_name}` of `{name}` cannot accept this value"),
                    )
                    .located(file, 1, None::<String>));
                }
            }
            for field in &struct_def.fields {
                if !field.is_optional && !provided.contains(&field.name.as_str()) {
                    return Err(Diagnostic::blocker(
                        "E1010",
                        format!("missing required field `{}` for `{name}`", field.name),
                    )
                    .located(
                        file,
                        1,
                        Some("pass the field as a keyword argument"),
                    ));
                }
            }
            Ok(())
        }
        Expr::Ident(var) => match params.get(var) {
            Some(ty) if ty == &TypeRef::Named(name.to_string()) => Ok(()),
            Some(_) => Err(Diagnostic::blocker(
                "E1010",
                format!("parameter `{var}` does not have the response type `{name}`"),
            )
            .located(
                file,
                1,
                Some("return a constructed DTO or change the parameter type"),
            )),
            None => Err(unknown_parameter(var, file)),
        },
        _ => Err(
            Diagnostic::blocker("E1010", format!("handler must return a `{name}` value")).located(
                file,
                1,
                Some(format!("construct it: `{name}(field=\"value\")`")),
            ),
        ),
    }
}

/// Whether the argument expression belongs to the field's type family.
/// Returns `Err` with a precise message only for shape errors (for example a
/// nested DTO construction where one is impossible); type mismatches return
/// `Ok(false)` and get a generic message from the caller.
fn arg_value_matches(
    value: &Expr,
    field_ty: &TypeRef,
    file: &str,
    params: &HashMap<String, TypeRef>,
) -> Result<bool, Diagnostic> {
    match value {
        Expr::Null => Ok(field_ty == &TypeRef::Json),
        Expr::Str(_) => Ok(matches!(field_ty, TypeRef::String | TypeRef::Json)),
        Expr::Int(_) => Ok(matches!(
            field_ty,
            TypeRef::Int | TypeRef::Float | TypeRef::Json
        )),
        Expr::Float(_) => Ok(matches!(field_ty, TypeRef::Float | TypeRef::Json)),
        Expr::Bool(_) => Ok(matches!(field_ty, TypeRef::Bool | TypeRef::Json)),
        Expr::Array(items) => match field_ty {
            TypeRef::Array { element, .. } => {
                for item in items {
                    if !arg_value_matches(item, element, file, params)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            TypeRef::Json => {
                for item in items {
                    if matches!(item, Expr::Construct { .. }) {
                        return Err(Diagnostic::blocker(
                            "E1010",
                            "constructing a DTO inside a list is not supported yet",
                        )
                        .located(file, 1, None::<String>));
                    }
                }
                Ok(true)
            }
            _ => Ok(false),
        },
        Expr::Object(_) => Ok(field_ty == &TypeRef::Json),
        Expr::Ident(var) => match params.get(var) {
            Some(ty) => Ok(ty == field_ty
                || field_ty == &TypeRef::Json
                || (field_ty == &TypeRef::Float && ty == &TypeRef::Int)),
            None => Err(unknown_parameter(var, file)),
        },
        Expr::Construct { ty, .. } => {
            if field_ty == &TypeRef::Named(ty.clone()) {
                Ok(true)
            } else {
                Err(Diagnostic::blocker(
                    "E1010",
                    format!(
                        "field expects `{}`, not a constructed `{ty}`",
                        type_label(field_ty)
                    ),
                )
                .located(file, 1, None::<String>))
            }
        }
    }
}

fn unknown_parameter(var: &str, file: &str) -> Diagnostic {
    Diagnostic::blocker(
        "E1010",
        format!("`{var}` is not a parameter of this handler"),
    )
    .located(file, 1, None::<String>)
}

/// Reject references to DTO types that are not defined as annotation-only
/// classes.
pub(crate) fn ensure_known_dtos(
    route: &RouteDefinition,
    dtos: &HashMap<String, StructDefinition>,
    file: &str,
) -> Result<(), Diagnostic> {
    fn check(
        ty: &TypeRef,
        dtos: &HashMap<String, StructDefinition>,
        file: &str,
    ) -> Result<(), Diagnostic> {
        match ty {
            TypeRef::Named(name) if !dtos.contains_key(name) => Err(Diagnostic::blocker(
                "E1003",
                format!("type `{name}` is not a defined DTO class"),
            )
            .located(
                file,
                1,
                Some("define the DTO as an annotation-only class in the app module"),
            )),
            TypeRef::Array { element, .. } => check(element, dtos, file),
            _ => Ok(()),
        }
    }
    if let RequestSpec::Json { ty, .. } = &route.request {
        check(ty, dtos, file)?;
    }
    if let ResponseSpec::Json(ty) = &route.response {
        check(ty, dtos, file)?;
    }
    Ok(())
}

/// The DTOs reachable from route request/response types, closed over nested
/// field references, in declaration order.
pub(crate) fn reachable_structs(
    routes: &[RouteDefinition],
    dtos: &HashMap<String, StructDefinition>,
    dto_order: &[String],
    file: &str,
) -> Result<Vec<StructDefinition>, Diagnostic> {
    fn named_refs_in_type(ty: &TypeRef, out: &mut Vec<String>) {
        match ty {
            TypeRef::Named(name) => out.push(name.clone()),
            TypeRef::Array { element, .. } => named_refs_in_type(element, out),
            _ => {}
        }
    }

    let mut queue: Vec<String> = Vec::new();
    for route in routes {
        if let RequestSpec::Json { ty, .. } = &route.request {
            named_refs_in_type(ty, &mut queue);
        }
        if let ResponseSpec::Json(ty) = &route.response {
            named_refs_in_type(ty, &mut queue);
        }
    }

    let mut reachable: Vec<String> = Vec::new();
    while let Some(name) = queue.pop() {
        if reachable.contains(&name) {
            continue;
        }
        let struct_def = dtos.get(&name).ok_or_else(|| {
            Diagnostic::blocker("E1003", format!("type `{name}` is not a defined DTO class"))
                .located(
                    file,
                    1,
                    Some("define the DTO as an annotation-only class in the app module"),
                )
        })?;
        reachable.push(name.clone());
        for field in &struct_def.fields {
            named_refs_in_type(&field.type_ref, &mut queue);
        }
    }

    Ok(dto_order
        .iter()
        .filter(|name| reachable.contains(*name))
        .filter_map(|name| dtos.get(name).cloned())
        .collect())
}
