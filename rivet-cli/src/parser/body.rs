//! Handler bodies: the supported statement subset.
//!
//! Phase 0 accepts a docstring, `pass`/`...`, and exactly one `return`
//! statement. Anything else (assignments, control flow, expressions with
//! side effects) is rejected with a diagnostic; the IR carries only the
//! translated return value.

use crate::diagnostic::Diagnostic;
use crate::parser::expr;
use crate::parser::{NamedChildren, is_docstring, line_of, node_text};
use rivet_core::ir::Expr;
use tree_sitter::Node;

/// Parse the statements of a handler body into its return values.
///
/// Phase 0 enforces at most one `return`; bare `return` lowers to
/// [`Expr::Null`]. The caller decides what a `-> None` handler does with null
/// returns.
pub(crate) fn parse_handler_body(
    function: &Node<'_>,
    source: &str,
    file: &str,
) -> Result<Vec<Expr>, Diagnostic> {
    let body = function.child_by_field_name("body").ok_or_else(|| {
        Diagnostic::blocker(
            "E1006",
            "handler without a body",
            "give the handler a body containing a single `return` statement",
        )
        .located(file, line_of(function))
    })?;

    let mut returns = Vec::new();
    for statement in body.named_children_all() {
        let statement_line = line_of(&statement);
        match statement.kind() {
            "expression_statement" if is_docstring(&statement) => {}
            "pass_statement" | "comment" => {}
            "expression_statement" if statement_text(&statement, source) == "..." => {}
            "return_statement" => {
                if !returns.is_empty() {
                    return Err(Diagnostic::blocker(
                        "E1006",
                        "multiple return statements are not supported yet; phase 0 handlers return once",
                        "use a single return at the end of the handler",
                    )
                    .located(file, statement_line));
                }
                let children = statement.named_children_all();
                let value = match children.first() {
                    Some(expression) => {
                        if children.len() > 1 {
                            return Err(Diagnostic::blocker(
                                "E1007",
                                "a return statement must hold a single expression",
                                "return one expression; wrap multiple values in a list or dict, e.g. `return {\"a\": a, \"b\": b}`",
                            )
                            .located(file, statement_line));
                        }
                        expr::translate(expression, source)
                            .map_err(|diag| attach(diag, file, statement_line))?
                    }
                    None => Expr::Null,
                };
                returns.push(value);
            }
            other => {
                return Err(Diagnostic::blocker(
                    "E1006",
                    format!("`{other}` statements are not supported in handler bodies yet"),
                    "phase 0 handlers contain a single return of literals, request parameters, or a DTO constructor",
                )
                .located(file, statement_line));
            }
        }
    }
    Ok(returns)
}

fn statement_text(node: &Node<'_>, source: &str) -> String {
    node_text(node, source).trim().to_string()
}

/// The expression translator reports a line but no file; anchor it to the
/// real file name here.
fn attach(diag: Diagnostic, file: &str, fallback_line: usize) -> Diagnostic {
    let mut diag = diag;
    if diag.file.is_none() {
        diag.file = Some(file.to_string());
    }
    if diag.line.is_none() {
        diag.line = Some(fallback_line);
    }
    diag
}
