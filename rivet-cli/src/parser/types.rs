//! DTO classes in the Python DSL.
//!
//! A DTO is an annotation-only class: its body holds field annotations,
//! docstrings, and `pass`/`...`. Methods or runtime statements make a class a
//! runtime object, which phase 0 cannot type, and route references to it are
//! rejected.

use crate::diagnostic::Diagnostic;
use crate::parser::annotation::parse_type_text;
use crate::parser::{NamedChildren, is_docstring, is_safe_identifier, line_of, node_text};
use rivet_core::ir::{FieldDefinition, StructDefinition};
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
                        let parsed = parse_type_text(
                            node_text(&type_node, source),
                            file,
                            statement_line,
                            false,
                        )?;
                        fields.push(FieldDefinition {
                            name: field_name.to_string(),
                            type_ref: parsed.type_ref,
                            is_optional: parsed.is_optional,
                            is_borrowed: parsed.is_borrowed,
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
