//! DTO reachability: which structs a blueprint's routes actually name.
//!
//! Separate from return validation because it answers a different question.
//! Return validation asks whether one handler's body fits its declared type;
//! this module asks which DTOs the blueprint must carry at all, closing over
//! nested field references.

use crate::diagnostic::Diagnostic;
use rivet_core::ir::{RequestSpec, ResponseSpec, RouteDefinition, StructDefinition, TypeRef};
use std::collections::HashMap;

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
                "define the DTO as an annotation-only class in the app module",
            )
            .located(file, 1)),
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
            Diagnostic::blocker(
                "E1003",
                format!("type `{name}` is not a defined DTO class"),
                "define the DTO as an annotation-only class in the app module",
            )
            .located(file, 1)
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
