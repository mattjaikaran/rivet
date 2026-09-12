//! Lowering of Python handler-body expressions into IR [`Expr`] values.
//!
//! The supported subset is a documented slice of Python expressions:
//!
//! - literals: `None`, `True`, `False`, integers, floats, strings, lists,
//!   dictionaries
//! - references to a handler parameter or a local the body assigned
//! - DTO construction: `OrderResponse(status="ok")`
//! - arithmetic (`+ - * /`), comparisons (`== != < <= > >=`), `and`,
//!   `or`, and `not`
//!
//! Anything else (attribute access, subscripts, calls other than a DTO
//! constructor, comprehensions, f-strings, ...) fails with a diagnostic that
//! names the construct. Failing loudly is deliberate: a silent
//! mistranslation of business logic would be worse than an error.

use crate::diagnostic::Diagnostic;
mod operator;
mod strings;

pub use strings::decode_string;

use operator::{translate_binary, translate_boolean, translate_comparison, translate_unary};

use crate::parser::NamedChildren;
use rivet_core::ir::Expr;
use tree_sitter::Node;

/// Translate a Python expression node into an IR expression.
///
/// Diagnostics carry a line but no file; the caller re-anchors them to the
/// real file name.
pub fn translate(node: &Node<'_>, source: &str) -> Result<Expr, Diagnostic> {
    match node.kind() {
        "none" => Ok(Expr::Null),
        "true" => Ok(Expr::Bool(true)),
        "false" => Ok(Expr::Bool(false)),
        "integer" => {
            let text = text(node, source);
            text.parse::<i64>().map(Expr::Int).map_err(|_| {
                at(
                    node,
                    "E1007",
                    format!("integer literal `{text}` is too large for the target"),
                    "use an integer literal within the supported range or represent the value as a string",
                )
            })
        }
        "float" => {
            let text = normalize_float(text(node, source));
            match text.parse::<f64>() {
                Ok(value) if value.is_finite() => Ok(Expr::Float(value)),
                Ok(_) => Err(at(
                    node,
                    "E1007",
                    format!("float literal `{text}` is not finite"),
                    "write a finite float literal, for example 1.5",
                )),
                Err(_) => Err(at(
                    node,
                    "E1007",
                    format!("float literal `{text}` is not valid"),
                    "write the float in a form Rust parses, for example 1.5 or 1e3",
                )),
            }
        }
        "string" => {
            let literal = text(node, source);
            match decode_string(literal) {
                Ok(value) => Ok(Expr::Str(value)),
                Err(reason) => Err(at(
                    node,
                    "E1007",
                    reason,
                    "rewrite the literal as a plain string without prefixes, f-strings, or invalid escapes",
                )),
            }
        }
        "concatenated_string" => Err(unsupported(
            node,
            "adjacent string literals are not supported; use a single literal",
        )),
        "list" => {
            let mut items = Vec::new();
            for child in node.named_children_all() {
                items.push(translate(&child, source)?);
            }
            Ok(Expr::Array(items))
        }
        "dictionary" => {
            let mut pairs = Vec::new();
            for child in node.named_children_all() {
                if child.kind() != "pair" {
                    return Err(unsupported(
                        node,
                        "dictionary unpacking (`**`) is not supported",
                    ));
                }
                let key = child.child_by_field_name("key").ok_or_else(|| {
                    at(
                        &child,
                        "E1007",
                        "dictionary entry without a key",
                        "add a string key to each dictionary entry, for example {\"key\": value}",
                    )
                })?;
                let value = child.child_by_field_name("value").ok_or_else(|| {
                    at(
                        &child,
                        "E1007",
                        "dictionary entry without a value",
                        "add a value to each dictionary entry, for example {\"key\": value}",
                    )
                })?;
                pairs.push((string_key(&key, source)?, translate(&value, source)?));
            }
            Ok(Expr::Object(pairs))
        }
        "identifier" => Ok(Expr::Ident(text(node, source).to_string())),
        "call" => {
            let function = node.child_by_field_name("function").ok_or_else(|| {
                at(
                    node,
                    "E1007",
                    "call without a callee",
                    "write a named DTO constructor call, for example OrderResponse(status=\"ok\")",
                )
            })?;
            if function.kind() != "identifier" {
                return Err(unsupported(
                    node,
                    "only DTO constructors may be called; calls on objects and modules are not supported yet",
                ));
            }
            let ty = text(&function, source).to_string();
            let mut args = Vec::new();
            if let Some(arguments) = node.child_by_field_name("arguments") {
                for child in arguments.named_children_all() {
                    if child.kind() != "keyword_argument" {
                        return Err(unsupported(
                            &child,
                            "DTO constructors take keyword arguments only, e.g. OrderResponse(status=\"ok\")",
                        ));
                    }
                    let name = child
                        .child_by_field_name("name")
                        .map(|n| text(&n, source))
                        .ok_or_else(|| {
                            at(
                                &child,
                                "E1007",
                                "keyword argument without a name",
                                "name each DTO constructor argument, for example OrderResponse(status=\"ok\")",
                            )
                        })?;
                    let value = child.child_by_field_name("value").ok_or_else(|| {
                        at(
                            &child,
                            "E1007",
                            "keyword argument without a value",
                            "give each DTO constructor argument a value, for example OrderResponse(status=\"ok\")",
                        )
                    })?;
                    args.push((name.to_string(), translate(&value, source)?));
                }
            }
            Ok(Expr::Construct { ty, args })
        }
        "unary_operator" | "not_operator" => translate_unary(node, source),
        "binary_operator" => translate_binary(node, source),
        "comparison_operator" => translate_comparison(node, source),
        "boolean_operator" => translate_boolean(node, source),
        "parenthesized_expression" => {
            let inner = node.named_children_all().first().copied().ok_or_else(|| {
                at(
                    node,
                    "E1007",
                    "empty parentheses",
                    "write a value inside the parentheses, or remove them",
                )
            })?;
            translate(&inner, source)
        }
        "attribute" => Err(unsupported(
            node,
            "attribute access (`x.y`) is not supported in handler bodies yet",
        )),
        other => Err(unsupported(
            node,
            &format!("the `{other}` expression is not part of the supported subset"),
        )),
    }
}

/// A dictionary key must be a string literal.
fn string_key(node: &Node<'_>, source: &str) -> Result<String, Diagnostic> {
    if node.kind() == "string" {
        return decode_string(text(node, source)).map_err(|reason| {
            at(
                node,
                "E1007",
                reason,
                "use a plain string literal without prefixes or invalid escapes as the dictionary key",
            )
        });
    }
    Err(unsupported(node, "dictionary keys must be string literals"))
}

/// Build a diagnostic anchored at a node's line.
fn at(node: &Node<'_>, code: &str, message: impl Into<String>, fix: &str) -> Diagnostic {
    let mut diagnostic = Diagnostic::blocker(code, message, fix);
    diagnostic.line = Some(node.start_position().row + 1);
    diagnostic
}

fn unsupported(node: &Node<'_>, reason: &str) -> Diagnostic {
    at(
        node,
        "E1007",
        reason.to_string(),
        "simplify the expression to the supported subset: literals, lists, dictionaries, parameters and locals, DTO constructors, and the arithmetic, comparison, and boolean operators",
    )
}

fn text<'a>(node: &Node<'_>, source: &'a str) -> &'a str {
    node.utf8_text(source.as_bytes()).unwrap_or("?")
}

/// Python accepts `1e3` and `.5`; Rust accepts the former but not a leading
/// dot. Normalize to text Rust can parse.
fn normalize_float(literal: &str) -> String {
    if literal.starts_with('.') {
        format!("0{literal}")
    } else {
        literal.to_string()
    }
}

#[cfg(test)]
mod tests;
