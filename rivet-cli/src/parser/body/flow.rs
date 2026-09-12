//! Flow statements in a handler body: `for` loops and `match` arms.
//!
//! Both statements open a block, and a block owns the bindings it declares.
//! [`super::parse_block`] drops every name a block introduced, so a statement
//! below a `for` or a `match` cannot read a name the block bound. Rust scopes
//! that `let` to the block, so a name that escaped would not compile.

use crate::diagnostic::Diagnostic;
use crate::parser::{NamedChildren, line_of, node_text};
use rivet_core::infer;
use rivet_core::ir::{Expr, Stmt, TypeRef};
use std::collections::HashMap;
use tree_sitter::Node;

use super::{parse_block, translate};

/// Parse `for <name> in <iterable>:`.
///
/// The loop binds `name` for its body alone. Nothing below the loop can read
/// the name, because Rust scopes the binding to the loop and a name that
/// escaped it would not compile. The body never reassigns the binding, so the
/// generated `for` needs no `mut`.
pub(super) fn parse_for(
    statement: &Node<'_>,
    source: &str,
    file: &str,
    env: &mut HashMap<String, TypeRef>,
) -> Result<Stmt, Diagnostic> {
    let line = line_of(statement);
    if let Some(alternative) = statement.child_by_field_name("alternative") {
        return Err(Diagnostic::blocker(
            "E1006",
            "a `for` ... `else` clause is not supported in handler bodies yet",
            "move the `else` body after the loop, or guard it with an `if`",
        )
        .located(file, line_of(&alternative)));
    }
    let target = statement.child_by_field_name("left").ok_or_else(|| {
        Diagnostic::blocker(
            "E1007",
            "`for` without a loop name",
            "name the loop binding, for example `for line in [1, 2, 3]:`",
        )
        .located(file, line)
    })?;
    if target.kind() != "identifier" {
        return Err(Diagnostic::blocker(
            "E1007",
            format!(
                "the loop binds `{}`; unpacking a loop value into several names is not supported yet",
                node_text(&target, source).trim()
            ),
            "loop over a list and bind one name, for example `for line in [1, 2, 3]:`",
        )
        .located(file, line));
    }
    let name = node_text(&target, source);
    if !crate::parser::is_safe_identifier(name) {
        return Err(Diagnostic::blocker(
            "E1011",
            format!("loop name `{name}` is not a safe Rust identifier"),
            "rename the loop binding to a snake_case Rust-safe identifier",
        )
        .located(file, line));
    }
    if env.contains_key(name) {
        return Err(Diagnostic::blocker(
            "E1016",
            format!(
                "the loop name `{name}` is already in scope; Python would keep the last element after the loop where Rust would keep the outer value"
            ),
            format!("rename the loop binding, for example `for each_{name} in ...`"),
        )
        .located(file, line));
    }

    let iterable_node = statement.child_by_field_name("right").ok_or_else(|| {
        Diagnostic::blocker(
            "E1007",
            "`for` without a collection",
            "give the loop a list, for example `for line in [1, 2, 3]:`",
        )
        .located(file, line)
    })?;
    let iterable = translate(&iterable_node, source, file, line)?;
    let iterable_ty = infer::infer(&iterable, env).map_err(|error| {
        Diagnostic::blocker(
            "E1010",
            format!(
                "cannot resolve the type of the loop's collection: {}",
                error.message()
            ),
            "loop over a list literal, or over a parameter declared as `List[...]`",
        )
        .located(file, line)
    })?;
    let TypeRef::Array { element, .. } = iterable_ty else {
        return Err(Diagnostic::blocker(
            "E1010",
            format!(
                "the loop's collection has type `{}`; a `for` needs a `list`",
                iterable_ty.label()
            ),
            "loop over a list literal, or over a parameter declared as `List[...]`",
        )
        .located(file, line));
    };

    let body_node = statement.child_by_field_name("body").ok_or_else(|| {
        Diagnostic::blocker(
            "E1007",
            "`for` without a body",
            "give the loop a body, for example `for line in [1, 2, 3]:\\n    total = total + line`",
        )
        .located(file, line)
    })?;
    let ty = *element;
    env.insert(name.to_string(), ty.clone());
    let body = parse_block(&body_node, source, file, env)?;
    env.remove(name);

    if Stmt::walk(&body)
        .into_iter()
        .any(|stmt| matches!(stmt, Stmt::Assign { name: bound, .. } if bound == name))
    {
        return Err(Diagnostic::blocker(
            "E1016",
            format!("the loop body assigns `{name}`, which is the loop's own binding"),
            "assign to a different local, or rename the loop binding",
        )
        .located(file, line));
    }

    Ok(Stmt::For {
        name: name.to_string(),
        ty,
        iterable,
        body,
    })
}

/// Parse `match <subject>:` with its `case` arms.
///
/// Every arm compares a literal with the subject, so one arm holds one literal
/// pattern and an optional wildcard. The wildcard must come last and must be
/// present: Rust rejects a `match` that does not cover every value, so a
/// missing wildcard would surface as a cargo error inside the generated crate
/// instead of as a diagnostic here.
pub(super) fn parse_match(
    statement: &Node<'_>,
    source: &str,
    file: &str,
    env: &mut HashMap<String, TypeRef>,
) -> Result<Stmt, Diagnostic> {
    let line = line_of(statement);
    let subject_node = statement.child_by_field_name("subject").ok_or_else(|| {
        Diagnostic::blocker(
            "E1007",
            "`match` without a subject",
            "give the `match` a value, for example `match score:`",
        )
        .located(file, line)
    })?;
    let subject = translate(&subject_node, source, file, line)?;
    let subject_ty = infer::infer(&subject, env).map_err(|error| {
        Diagnostic::blocker(
            "E1010",
            format!(
                "cannot resolve the type of the `match` subject: {}",
                error.message()
            ),
            "match on a number, a string, or a boolean, or on a parameter of one of those types",
        )
        .located(file, line)
    })?;
    if !matches!(
        subject_ty,
        TypeRef::Int | TypeRef::Float | TypeRef::Bool | TypeRef::String
    ) {
        return Err(Diagnostic::blocker(
            "E1010",
            format!(
                "the `match` subject has type `{}`; a `match` needs an `int`, a `float`, a `str`, or a `bool`",
                subject_ty.label()
            ),
            "match on a number, a string, or a boolean; a `dict` value has no literal pattern the generated crate can compare",
        )
        .located(file, line));
    }

    let body_node = statement.child_by_field_name("body").ok_or_else(|| {
        Diagnostic::blocker(
            "E1007",
            "`match` without a body",
            "give the `match` at least one `case` arm and a final `case _:`",
        )
        .located(file, line)
    })?;

    let mut arms: Vec<(Option<Expr>, Vec<Stmt>)> = Vec::new();
    for clause in body_node.named_children_all() {
        if clause.kind() != "case_clause" {
            continue;
        }
        let clause_line = line_of(&clause);
        if clause.child_by_field_name("guard").is_some() {
            return Err(Diagnostic::blocker(
                "E1006",
                "a `case` guard (`case x if <condition>:`) is not supported in handler bodies yet",
                "split the guard into its own `if` inside the arm's body",
            )
            .located(file, clause_line));
        }
        let pattern_node = clause
            .named_children_all()
            .into_iter()
            .find(|child| child.kind() == "case_pattern")
            .ok_or_else(|| {
                Diagnostic::blocker(
                    "E1007",
                    "`case` without a pattern",
                    "write a literal pattern, for example `case 1:`, or the wildcard `case _:`",
                )
                .located(file, clause_line)
            })?;
        let pattern = pattern_of(&pattern_node, &subject_ty, source, file, clause_line)?;
        if pattern
            .as_ref()
            .is_some_and(|pattern| arms.iter().any(|(seen, _)| seen.as_ref() == Some(pattern)))
        {
            return Err(Diagnostic::blocker(
                "E1017",
                format!(
                    "the pattern `{}` appears in an earlier `case`",
                    node_text(&pattern_node, source).trim()
                ),
                "remove the repeated `case`, or change it to a value no earlier arm matches",
            )
            .located(file, clause_line));
        }
        let consequence = clause.child_by_field_name("consequence").ok_or_else(|| {
            Diagnostic::blocker(
                "E1007",
                "`case` without a body",
                "give the arm a body, for example `case 1:\\n    return {\"grade\": \"low\"}`",
            )
            .located(file, clause_line)
        })?;
        arms.push((pattern, parse_block(&consequence, source, file, env)?));
    }

    match arms.iter().position(|(pattern, _)| pattern.is_none()) {
        None => {
            return Err(Diagnostic::blocker(
                "E1017",
                "the `match` has no `case _` wildcard",
                "add a final `case _:` arm; Rust rejects a `match` that does not cover every value",
            )
            .located(file, line));
        }
        Some(index) if index + 1 != arms.len() => {
            return Err(Diagnostic::blocker(
                "E1017",
                "the `case _` wildcard is not the last arm",
                "move `case _:` below every other `case`, because it matches every value",
            )
            .located(file, line));
        }
        Some(_) => {}
    }

    Ok(Stmt::Match { subject, arms })
}

/// The literal an arm compares, or `None` for the `case _` wildcard.
///
/// The grammar separates a leading minus from the number it negates, so the
/// node for `case -1:` holds a positive `integer` and the sign sits outside
/// it. Reading the number alone would answer `1` for `-1`, so the pattern's
/// own text is the one that decides the sign.
fn pattern_of(
    pattern: &Node<'_>,
    subject: &TypeRef,
    source: &str,
    file: &str,
    line: usize,
) -> Result<Option<Expr>, Diagnostic> {
    let children = pattern.named_children_all();
    let Some(literal) = children.first().copied() else {
        return Ok(None);
    };
    if literal.kind() == "union_pattern" {
        return Err(Diagnostic::blocker(
            "E1017",
            format!(
                "the pattern `{}` lists several alternatives",
                node_text(pattern, source).trim()
            ),
            "write one literal per `case`, or add the extra values as further `case` arms",
        )
        .located(file, line));
    }
    let text = node_text(pattern, source).trim();
    let negated = text.starts_with('-');
    let mut expr = translate(&literal, source, file, line)?;
    if negated {
        expr = match expr {
            Expr::Int(value) => Expr::Int(-value),
            Expr::Float(value) => Expr::Float(-value),
            _ => {
                return Err(Diagnostic::blocker(
                    "E1017",
                    format!("the pattern `{text}` is not a literal number a `case` can compare"),
                    "write a literal pattern such as `case -1:` or `case _:`",
                )
                .located(file, line));
            }
        };
    }
    if !matches!(
        expr,
        Expr::Int(_) | Expr::Float(_) | Expr::Str(_) | Expr::Bool(_) | Expr::Null
    ) {
        return Err(Diagnostic::blocker(
            "E1017",
            format!(
                "the pattern `{text}` is not a literal; a `case` compares one literal by equality"
            ),
            "write a literal pattern such as `case 1:`, `case \"low\":`, or `case _:`",
        )
        .located(file, line));
    }
    let pattern_ty = infer::infer(&expr, &HashMap::new()).map_err(|error| {
        Diagnostic::blocker(
            "E1017",
            format!(
                "cannot resolve the type of the pattern `{text}`: {}",
                error.message()
            ),
            "write a literal pattern such as `case 1:`, `case \"low\":`, or `case _:`",
        )
        .located(file, line)
    })?;
    if pattern_ty != *subject {
        return Err(Diagnostic::blocker(
            "E1017",
            format!(
                "the pattern `{text}` has type `{}`; the subject has type `{}`",
                pattern_ty.label(),
                subject.label()
            ),
            "make every `case` a literal of the subject's own type",
        )
        .located(file, line));
    }
    Ok(Some(expr))
}
