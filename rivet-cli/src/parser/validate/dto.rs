//! DTO construction checks: does this return value build the declared DTO?
//!
//! Separate from the primitive and array checks because a DTO answer has
//! structure to verify — field names, optionality, and the shape of each
//! argument — rather than one leaf type to compare.

use super::unknown_parameter;
use crate::diagnostic::Diagnostic;
use rivet_core::infer;
use rivet_core::ir::{Expr, StructDefinition, TypeRef};
use std::collections::HashMap;

/// A DTO response must construct that DTO or return a parameter of that type.
pub(super) fn validate_construct(
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
                    format!("define `{name}` as an annotation-only class in the app module"),
                )
                .located(file, 1)
            })?;
            if ty != name {
                return Err(Diagnostic::blocker(
                    "E1010",
                    format!("handler must return `{name}`, not `{ty}`"),
                    format!("construct `{name}(field=\"value\")` instead of `{ty}`"),
                )
                .located(file, 1));
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
                        format!(
                            "available fields: {}",
                            struct_def
                                .fields
                                .iter()
                                .map(|f| f.name.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                    )
                    .located(file, 1));
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
                        "pass a value of the field's declared type or remove the argument",
                    )
                    .located(file, 1));
                }
            }
            for field in &struct_def.fields {
                if !field.is_optional && !provided.contains(&field.name.as_str()) {
                    return Err(Diagnostic::blocker(
                        "E1010",
                        format!("missing required field `{}` for `{name}`", field.name),
                        "pass the field as a keyword argument",
                    )
                    .located(file, 1));
                }
            }
            Ok(())
        }
        Expr::Ident(var) => match params.get(var) {
            Some(ty) if ty == &TypeRef::Named(name.to_string()) => Ok(()),
            Some(_) => Err(Diagnostic::blocker(
                "E1010",
                format!("parameter `{var}` does not have the response type `{name}`"),
                "return a constructed DTO or change the parameter type",
            )
            .located(file, 1)),
            None => Err(unknown_parameter(var, file)),
        },
        _ => Err(Diagnostic::blocker(
            "E1010",
            format!("handler must return a `{name}` value"),
            format!("construct it: `{name}(field=\"value\")`"),
        )
        .located(file, 1)),
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
        Expr::Binary { .. } | Expr::Not(_) => Ok(match infer::infer(value, params) {
            Ok(ty) => &ty == field_ty || field_ty == &TypeRef::Json,
            Err(_) => false,
        }),
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
                            "return plain values in the list, or construct the DTO only as the top-level response",
                        )
                        .located(file, 1));
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
                        field_ty.label()
                    ),
                    format!("pass a value of type `{}` for this field", field_ty.label()),
                )
                .located(file, 1))
            }
        }
    }
}
