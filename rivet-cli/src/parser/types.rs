//! Type annotations and DTO classes in the Python DSL.
//!
//! A DTO is an annotation-only class: its body holds field annotations,
//! docstrings, and `pass`/`...`. Methods or runtime statements make a class a
//! runtime object, which phase 0 cannot type, and route references to it are
//! rejected.

use crate::diagnostic::Diagnostic;
use crate::parser::{NamedChildren, is_docstring, is_safe_identifier, line_of, node_text};
use rivet_core::ir::{FieldDefinition, StructDefinition, TypeRef};
use tree_sitter::Node;

/// Parse a class definition into a DTO when it qualifies.
pub(crate) fn parse_dto_class(
    node: &Node<'_>,
    source: &str,
    file: &str,
) -> Result<Option<StructDefinition>, Diagnostic> {
    let name_node = node.child_by_field_name("name").ok_or_else(|| {
        Diagnostic::blocker(
            "E1003",
            "class without a name",
            "name the class, for example `class Order:`",
        )
        .located(file, line_of(node))
    })?;
    let name = node_text(&name_node, source);
    if !is_safe_identifier(name) {
        return Err(Diagnostic::blocker(
            "E1011",
            format!("class name `{name}` is not a safe Rust identifier"),
            "rename the class to a PascalCase Rust-safe identifier, for example `OrderCreate`",
        )
        .located(file, line_of(node)));
    }

    let body = node.child_by_field_name("body").ok_or_else(|| {
        Diagnostic::blocker(
            "E1003",
            "class without a body",
            "give the class a body of field annotations, for example a class body containing `sku: str`",
        )
        .located(file, line_of(node))
    })?;

    let mut fields = Vec::new();
    for statement in body.named_children_all() {
        let statement_line = line_of(&statement);
        match statement.kind() {
            "expression_statement" => {
                if is_docstring(&statement) {
                    continue;
                }
                match annotation_field(&statement, source) {
                    Some((field_name, type_node)) => {
                        if !is_safe_identifier(field_name) {
                            return Err(Diagnostic::blocker(
                                "E1011",
                                format!("field name `{field_name}` is not a safe Rust identifier"),
                                "rename the field to a snake_case Rust-safe identifier",
                            )
                            .located(file, statement_line));
                        }
                        let (type_ref, is_optional) = parse_type_text(
                            node_text(&type_node, source),
                            file,
                            statement_line,
                            false,
                        )?;
                        fields.push(FieldDefinition {
                            name: field_name.to_string(),
                            type_ref,
                            is_optional,
                            is_borrowed: false,
                        });
                    }
                    None => return Ok(None), // runtime class, not a DTO
                }
            }
            "pass_statement" | "comment" => {}
            _ => {
                // Methods, assignments, and other statements disqualify the
                // class as a phase-0 DTO.
                return Ok(None);
            }
        }
    }
    Ok(Some(StructDefinition {
        name: name.to_string(),
        fields,
    }))
}

/// For an annotation statement `name: Type`, return the field name and the
/// `type` node. Returns `None` for any other statement shape.
fn annotation_field<'t>(statement: &'t Node<'t>, source: &'t str) -> Option<(&'t str, Node<'t>)> {
    for child in statement.named_children_all() {
        if child.kind() != "assignment" {
            continue;
        }
        let left = child.child_by_field_name("left");
        let ty = child.child_by_field_name("type");
        let (Some(left), Some(ty)) = (left, ty) else {
            return None;
        };
        if left.kind() != "identifier" {
            return None;
        }
        return Some((node_text(&left, source), ty));
    }
    None
}

/// Parse a type annotation into a [`TypeRef`].
///
/// Accepts optional wrappers (`Optional[X]`, `X | None`), `dict`/primitives,
/// `List[T]` / `List[T, N]`, and references to DTO classes. DTO references
/// are not resolved here; the caller holds the class table.
pub(crate) fn parse_type_text(
    annotation: &str,
    file: &str,
    line: usize,
    inside_array: bool,
) -> Result<(TypeRef, bool), Diagnostic> {
    let text = annotation.trim();

    // `Optional[X]`
    if text.starts_with("Optional[") && text.ends_with(']') {
        let inner = &text[9..text.len() - 1];
        let (type_ref, _) = parse_type_text(inner, file, line, inside_array)?;
        return Ok((type_ref, true));
    }
    // `X | None` unions
    if let Some(inner) = text.strip_suffix("| None").map(str::trim) {
        let (type_ref, _) = parse_type_text(inner, file, line, inside_array)?;
        return Ok((type_ref, true));
    }
    if let Some(inner) = text.strip_suffix("None |").map(str::trim) {
        let (type_ref, _) = parse_type_text(inner, file, line, inside_array)?;
        return Ok((type_ref, true));
    }
    if text.contains('|') {
        return Err(Diagnostic::blocker(
            "E1002",
            format!("union type `{text}` is not supported; use `Optional[...]`"),
            "rewrite the annotation with `Optional[...]` or a single type, for example `Optional[str]`",
        )
        .located(file, line));
    }

    // `List[T]` or `List[T, N]` (the DSL drops the `typing.` prefix)
    if let Some(inner) = array_inner(text) {
        if inside_array {
            return Err(Diagnostic::blocker(
                "E1002",
                format!("nested array type `{text}` is not supported"),
                "flatten the annotation to a single array level, for example `List[dict]`",
            )
            .located(file, line));
        }
        let parts: Vec<&str> = inner.split(',').map(str::trim).collect();
        let (element_text, len) = match parts.as_slice() {
            [element] => (*element, None),
            [element, size] => {
                let size = size.parse::<usize>().map_err(|_| {
                    Diagnostic::blocker(
                        "E1002",
                        format!("array size `{size}` is not a positive integer"),
                        "give the array a positive integer size, for example `List[float, 768]`",
                    )
                    .located(file, line)
                })?;
                (*element, Some(size))
            }
            _ => {
                return Err(Diagnostic::blocker(
                    "E1002",
                    format!("array type `{text}` must name an element type and an optional size"),
                    "write the array as `List[Element]` or `List[Element, Size]`, for example `List[str]`",
                )
                .located(file, line));
            }
        };
        let (element, _) = parse_type_text(element_text, file, line, true)?;
        return Ok((
            TypeRef::Array {
                element: Box::new(element),
                len,
            },
            false,
        ));
    }

    match text {
        "str" => Ok((TypeRef::String, false)),
        "bool" => Ok((TypeRef::Bool, false)),
        "int" => Ok((TypeRef::Int, false)),
        "float" => Ok((TypeRef::Float, false)),
        "dict" => Ok((TypeRef::Json, false)),
        "list" => Ok((
            TypeRef::Array {
                element: Box::new(TypeRef::Json),
                len: None,
            },
            false,
        )),
        other if other.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') => {
            // A DTO reference; existence is checked by the caller.
            if !is_safe_identifier(other) {
                return Err(Diagnostic::blocker(
                    "E1011",
                    format!("type name `{other}` is not a safe Rust identifier"),
                    "reference an existing DTO by a PascalCase name or use a builtin type",
                )
                .located(file, line));
            }
            Ok((TypeRef::Named(other.to_string()), false))
        }
        other => Err(Diagnostic::blocker(
            "E1002",
            format!("type annotation `{other}` is not supported"),
            "use str, bool, int, float, dict, List[T], Optional[T], or a DTO class",
        )
        .located(file, line)),
    }
}

/// If the annotation is `List[...]` (optionally `typing.List[...]`), return
/// the inner text.
fn array_inner(text: &str) -> Option<&str> {
    let marker = "List[";
    let start = text.find(marker)?;
    let prefix = &text[..start];
    if !prefix.is_empty() && !prefix.ends_with('.') {
        return None;
    }
    let tail = &text[start + marker.len()..];
    tail.strip_suffix(']')
}

/// A human label for a type, used in diagnostics.
pub(crate) fn type_label(ty: &TypeRef) -> String {
    match ty {
        TypeRef::String => "str".to_string(),
        TypeRef::Bool => "bool".to_string(),
        TypeRef::Int => "int".to_string(),
        TypeRef::Float => "float".to_string(),
        TypeRef::Json => "dict".to_string(),
        TypeRef::Array { .. } => "list".to_string(),
        TypeRef::Named(name) => name.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_optional_and_array_annotations() {
        assert_eq!(
            parse_type_text("List[float, 768]", "app.py", 1, false)
                .expect("parse")
                .0,
            TypeRef::Array {
                element: Box::new(TypeRef::Float),
                len: Some(768),
            }
        );
        assert_eq!(
            parse_type_text("Optional[int]", "app.py", 1, false).expect("parse"),
            (TypeRef::Int, true)
        );
        assert_eq!(
            parse_type_text("int | None", "app.py", 1, false).expect("parse"),
            (TypeRef::Int, true)
        );
        assert_eq!(
            parse_type_text("dict", "app.py", 1, false).expect("parse"),
            (TypeRef::Json, false)
        );
    }

    #[test]
    fn rejects_unknown_shapes() {
        let diagnostic = parse_type_text("Tuple[int]", "app.py", 1, false).expect_err("must fail");
        assert_eq!(diagnostic.error_code, "E1002");
        assert!(!diagnostic.suggested_fix.is_empty());
        let diagnostic =
            parse_type_text("List[List[int]]", "app.py", 1, false).expect_err("must fail");
        assert_eq!(diagnostic.error_code, "E1002");
        assert!(!diagnostic.suggested_fix.is_empty());
        let diagnostic = parse_type_text("int | str", "app.py", 1, false).expect_err("must fail");
        assert_eq!(diagnostic.error_code, "E1002");
        assert!(!diagnostic.suggested_fix.is_empty());
    }
}
