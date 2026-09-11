//! The infix and unary operators of a handler expression.
//!
//! Each translator reads one grammar node and returns one IR node. Whether the
//! operands can combine is not decided here: only the caller knows the
//! handler's parameters and locals, so the type rule lives in
//! `rivet_core::infer`.

use super::{at, text, translate, unsupported};
use crate::diagnostic::Diagnostic;
use crate::parser::NamedChildren;
use rivet_core::ir::{BinOp, Expr};
use tree_sitter::Node;

/// Translate a unary operator: a negated numeric literal, or `not`.
pub(super) fn translate_unary(node: &Node<'_>, source: &str) -> Result<Expr, Diagnostic> {
    let operator = node.child(0).map(|c| text(&c, source)).unwrap_or("");
    let operand = node.named_children_all().first().copied().ok_or_else(|| {
        at(
            node,
            "E1007",
            "unary operator without an operand",
            "place the operator before a value, for example -1 or -1.5",
        )
    })?;
    match operator {
        "-" => match translate(&operand, source)? {
            Expr::Int(value) => Ok(Expr::Int(-value)),
            Expr::Float(value) => Ok(Expr::Float(-value)),
            _ => Err(unsupported(
                node,
                "negation is supported for numeric literals only",
            )),
        },
        "+" => translate(&operand, source),
        "not" => Ok(Expr::Not(Box::new(translate(&operand, source)?))),
        other => Err(unsupported(
            node,
            &format!("the unary operator `{other}` is not supported"),
        )),
    }
}

/// Translate `a + b` and the other infix operators that are not comparisons.
///
/// The operator decides the IR node; whether the operands' types can combine
/// is a later check, because only the callers know the handler's parameters
/// and locals.
pub(super) fn translate_binary(node: &Node<'_>, source: &str) -> Result<Expr, Diagnostic> {
    let left = node.child_by_field_name("left").ok_or_else(|| {
        at(
            node,
            "E1007",
            "operator without a left operand",
            "write a value on both sides of the operator",
        )
    })?;
    let right = node.child_by_field_name("right").ok_or_else(|| {
        at(
            node,
            "E1007",
            "operator without a right operand",
            "write a value on both sides of the operator",
        )
    })?;
    let operator = node
        .child_by_field_name("operator")
        .map(|n| text(&n, source))
        .unwrap_or("");
    let op = match operator {
        "+" => BinOp::Add,
        "-" => BinOp::Sub,
        "*" => BinOp::Mul,
        "/" => BinOp::Div,
        // Python floors toward negative infinity and gives the remainder the
        // divisor's sign; Rust truncates and gives it the dividend's. `-7 // 2`
        // is -4 in Python and -3 in Rust, and `-7 % 3` is 2 against -1. A
        // translation would answer differently for every negative operand, so
        // the front end refuses rather than pick one meaning.
        "//" | "%" => {
            return Err(at(
                node,
                "E1007",
                format!(
                    "the operator `{operator}` means something different in Rust than in Python for negative values"
                ),
                "use `*`, `/`, `+`, or `-`; for a remainder that is always positive, subtract the right multiple of the divisor instead",
            ));
        }
        other => {
            return Err(unsupported(
                node,
                &format!("the operator `{other}` is not supported in handler expressions"),
            ));
        }
    };
    Ok(Expr::Binary {
        op,
        left: Box::new(translate(&left, source)?),
        right: Box::new(translate(&right, source)?),
    })
}

/// Translate `a < b`.
///
/// The grammar gives a comparison an `operators` field and a flat list of
/// operands, with no `left`/`right`, because Python chains them: `a < b < c`
/// means `a < b and b < c`, not `(a < b) < c`. This lowers the chain to that
/// conjunction, which is what makes the meaning survive.
pub(super) fn translate_comparison(node: &Node<'_>, source: &str) -> Result<Expr, Diagnostic> {
    // The grammar exposes the operators through a repeated field and leaves
    // the operands as plain children, so read the children in order and split
    // them: an operator is an anonymous token, an operand is a named node.
    let mut operands: Vec<Node<'_>> = Vec::new();
    let mut operators: Vec<String> = Vec::new();
    for index in 0..node.child_count() {
        let Some(child) = node.child(index) else {
            continue;
        };
        if child.is_named() {
            operands.push(child);
        } else {
            operators.push(text(&child, source).to_string());
        }
    }
    if operators.is_empty() || operands.len() != operators.len() + 1 {
        return Err(unsupported(
            node,
            "a comparison must join two values with `==`, `!=`, `<`, `<=`, `>`, or `>=`",
        ));
    }

    // Fold the chain from the right, so `a < b < c` becomes `a < b and b < c`.
    // The middle operand is emitted twice, which is what the chain means.
    let mut folded: Option<Expr> = None;
    for index in (0..operators.len()).rev() {
        let op = match operators[index].as_str() {
            "==" => BinOp::Eq,
            "!=" => BinOp::NotEq,
            "<" => BinOp::Lt,
            "<=" => BinOp::LtEq,
            ">" => BinOp::Gt,
            ">=" => BinOp::GtEq,
            other => {
                return Err(unsupported(
                    node,
                    &format!("the comparison `{other}` is not supported"),
                ));
            }
        };
        let comparison = Expr::Binary {
            op,
            left: Box::new(translate(&operands[index], source)?),
            right: Box::new(translate(&operands[index + 1], source)?),
        };
        folded = Some(match folded {
            None => comparison,
            Some(tail) => Expr::Binary {
                op: BinOp::And,
                left: Box::new(comparison),
                right: Box::new(tail),
            },
        });
    }
    folded.ok_or_else(|| unsupported(node, "empty comparison"))
}

/// Translate `a and b` / `a or b`.
pub(super) fn translate_boolean(node: &Node<'_>, source: &str) -> Result<Expr, Diagnostic> {
    let left = node.child_by_field_name("left").ok_or_else(|| {
        at(
            node,
            "E1007",
            "boolean operator without a left operand",
            "write a value on both sides of `and` or `or`",
        )
    })?;
    let right = node.child_by_field_name("right").ok_or_else(|| {
        at(
            node,
            "E1007",
            "boolean operator without a right operand",
            "write a value on both sides of `and` or `or`",
        )
    })?;
    let operator = node
        .child_by_field_name("operator")
        .map(|n| text(&n, source))
        .unwrap_or("");
    let op = match operator {
        "and" => BinOp::And,
        "or" => BinOp::Or,
        other => {
            return Err(unsupported(
                node,
                &format!("the boolean operator `{other}` is not supported"),
            ));
        }
    };
    Ok(Expr::Binary {
        op,
        left: Box::new(translate(&left, source)?),
        right: Box::new(translate(&right, source)?),
    })
}
