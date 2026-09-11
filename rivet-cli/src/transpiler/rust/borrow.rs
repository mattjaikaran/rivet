//! Borrowed request bodies: the `zero_copy_deserialization` opt-in.
//!
//! A DTO field annotated `borrowed[str]` renders as `&'a str` behind
//! `#[serde(borrow)]`, so its DTO takes a lifetime and deserializes by
//! borrowing from the request body instead of copying each string. The
//! borrow has one place to live: the buffer that holds the request. The
//! generator therefore accepts a borrowed DTO **only** as a route's request
//! body, and this module holds both rules that keep it there:
//!
//! - [`check_opt_in`] rejects a borrow in a project that has not set the flag,
//!   so the config never claims a capability the build does not apply;
//! - [`check_positions`] rejects a borrowed DTO in a position where no buffer
//!   outlives the value — a response, or a field of another DTO.
//!
//! Both rules run before any crate is written, so the user reads a diagnostic
//! instead of a cargo error against generated code.

use crate::config::RivetConfig;
use crate::diagnostic::Diagnostic;
use rivet_core::ir::{ServiceBlueprint, StructDefinition, TypeRef};

/// The lifetime every borrowed DTO takes.
///
/// One lifetime is enough: a DTO's borrowed fields all point into the same
/// request body.
pub(super) const LIFETIME: &str = "'a";

/// Whether any field of `struct_def` borrows from the request body.
pub(super) fn has_borrowed_field(struct_def: &StructDefinition) -> bool {
    struct_def.fields.iter().any(|field| field.is_borrowed)
}

/// Reject a borrowed DTO the generator cannot render.
pub(super) fn check(blueprint: &ServiceBlueprint, config: &RivetConfig) -> Result<(), Diagnostic> {
    check_opt_in(blueprint, config)?;
    check_positions(blueprint)
}

/// Reject a borrow in a project that has not opted in.
///
/// The flag gates the rendering, so a `borrowed[str]` field without it is a
/// config that under-claims rather than a build that ignores the field. The
/// fix names the flag, exactly as `E2003` does for a fixed-size array.
fn check_opt_in(blueprint: &ServiceBlueprint, config: &RivetConfig) -> Result<(), Diagnostic> {
    if config.rust_native_features.zero_copy_deserialization {
        return Ok(());
    }
    for struct_def in &blueprint.structs {
        if let Some(field) = struct_def.fields.iter().find(|field| field.is_borrowed) {
            return Err(Diagnostic::blocker(
                "E2015",
                format!(
                    "`{}.{}` borrows from the request body, and `zero_copy_deserialization` is not set",
                    struct_def.name, field.name
                ),
                "set `zero_copy_deserialization = true` in the `[rust_native_features]` section of `rivet.toml`, or declare the field as `str` so it owns its text, then rerun the command",
            )
            .located("<generated>", 1));
        }
    }
    Ok(())
}

/// Reject a borrowed DTO outside a route's request body.
///
/// An owned position would need the borrow and the value to share one
/// lifetime, which the generator does not render: `Note<'_>` is written where
/// the lifetime is elided, and the generated crate would fail to compile with
/// a message about code the user never wrote.
fn check_positions(blueprint: &ServiceBlueprint) -> Result<(), Diagnostic> {
    for struct_def in blueprint
        .structs
        .iter()
        .filter(|struct_def| has_borrowed_field(struct_def))
    {
        let name = struct_def.name.as_str();
        if let Some(route) = blueprint.routes.iter().find(|route| match &route.response {
            rivet_core::ir::ResponseSpec::Json(ty) => names_dto(ty, name),
            rivet_core::ir::ResponseSpec::None => false,
        }) {
            return Err(Diagnostic::blocker(
                "E2016",
                format!(
                    "`{name}` borrows from the request body, so it cannot be the response type of `{} {}`",
                    route.method.as_str(),
                    route.path
                ),
                format!(
                    "return an owned DTO instead: declare `{name}`'s borrowed fields as `str`, or reply with another class, then rerun the command"
                ),
            )
            .located("<generated>", 1));
        }
        for other in &blueprint.structs {
            for field in &other.fields {
                if names_dto(&field.type_ref, name) {
                    return Err(Diagnostic::blocker(
                        "E2016",
                        format!(
                            "`{name}` borrows from the request body, so it cannot be the type of `{}.{}`",
                            other.name, field.name
                        ),
                        format!(
                            "declare `{}.{}` as an owned DTO, or declare `{name}`'s text fields as `str` so the request body outlives nothing, then rerun the command",
                            other.name, field.name
                        ),
                    )
                    .located("<generated>", 1));
                }
            }
        }
    }
    Ok(())
}

/// Whether `ty` names `name`, directly or as an array element.
fn names_dto(ty: &TypeRef, name: &str) -> bool {
    match ty {
        TypeRef::Named(other) => other == name,
        TypeRef::Array { element, .. } => names_dto(element, name),
        _ => false,
    }
}

#[cfg(test)]
mod tests;
