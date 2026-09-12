//! Handler bodies: the supported statement subset.
//!
//! A body is a sequence of statements. It may bind locals, branch on a
//! condition, loop over a list, match a value against literals, and return
//! from any block. Everything else is rejected with a diagnostic rather than
//! translated by guesswork.
//!
//! Two shapes are easy to read wrongly in this grammar:
//!
//! - an assignment is not a statement kind of its own. It is an
//!   `expression_statement` whose first named child is an `assignment`, so the
//!   admission check has to look one level down;
//! - `child_by_field_name("alternative")` on an `if_statement` answers only
//!   the first `elif`/`else`, so the alternatives are read as named children.
//!
//! A block's bindings live for that block alone. Python scopes a local to the
//! whole function, but Rust scopes the `let` to the block that holds it, so a
//! name an arm bound would not compile when a statement below the arm reads
//! it. [`parse_block`] therefore drops the names a block introduced.

mod flow;

use crate::diagnostic::Diagnostic;
use crate::parser::expr;
use crate::parser::{NamedChildren, is_docstring, line_of, node_text};
use rivet_core::infer;
use rivet_core::ir::{Expr, Stmt, TypeRef};
use std::collections::{HashMap, HashSet};
use tree_sitter::Node;

/// Parse a handler body into its statements.
///
/// `env` holds the handler's parameter types and gains a binding for every
/// local the body assigns, in source order, so a later statement sees the
/// types the earlier ones bound. A bare `return` lowers to [`Expr::Null`].
pub(crate) fn parse_handler_body(
    function: &Node<'_>,
    source: &str,
    file: &str,
    env: &mut HashMap<String, TypeRef>,
) -> Result<Vec<Stmt>, Diagnostic> {
    let body = function.child_by_field_name("body").ok_or_else(|| {
        Diagnostic::blocker(
            "E1006",
            "handler without a body",
            "give the handler a body that returns a value",
        )
        .located(file, line_of(function))
    })?;
    parse_block(&body, source, file, env)
}

/// Parse the statements of one block.
///
/// The bindings the block introduces disappear when the block ends, so a
/// statement below cannot read a name an `if`, `for`, or `match` block bound.
fn parse_block(
    block: &Node<'_>,
    source: &str,
    file: &str,
    env: &mut HashMap<String, TypeRef>,
) -> Result<Vec<Stmt>, Diagnostic> {
    let outer: HashSet<String> = env.keys().cloned().collect();
    let mut statements = Vec::new();
    for statement in block.named_children_all() {
        if let Some(parsed) = parse_statement(&statement, source, file, env)? {
            statements.push(parsed);
        }
    }
    env.retain(|name, _| outer.contains(name));
    Ok(statements)
}

/// Parse one statement, or answer `None` for a statement that carries no
/// meaning (a docstring, `pass`, `...`, a comment).
fn parse_statement(
    statement: &Node<'_>,
    source: &str,
    file: &str,
    env: &mut HashMap<String, TypeRef>,
) -> Result<Option<Stmt>, Diagnostic> {
    let line = line_of(statement);
    match statement.kind() {
        "expression_statement" if is_docstring(statement) => Ok(None),
        "pass_statement" | "comment" => Ok(None),
        "expression_statement" if node_text(statement, source).trim() == "..." => Ok(None),
        "expression_statement" => match statement.named_children_all().first().copied() {
            Some(assignment) if assignment.kind() == "assignment" => {
                parse_assignment(&assignment, source, file, env).map(Some)
            }
            _ => Err(unsupported(statement, source, file, line)),
        },
        "return_statement" => {
            let children = statement.named_children_all();
            let value = match children.first() {
                Some(expression) => {
                    if children.len() > 1 {
                        return Err(Diagnostic::blocker(
                            "E1007",
                            "a return statement must hold a single expression",
                            "return one expression; wrap multiple values in a list or dict, e.g. `return {\"a\": a, \"b\": b}`",
                        )
                        .located(file, line));
                    }
                    translate(expression, source, file, line)?
                }
                None => Expr::Null,
            };
            Ok(Some(Stmt::Return(value)))
        }
        "if_statement" => parse_if(statement, source, file, env).map(Some),
        "for_statement" => flow::parse_for(statement, source, file, env).map(Some),
        "match_statement" => flow::parse_match(statement, source, file, env).map(Some),
        other => Err(Diagnostic::blocker(
            "E1006",
            format!("`{other}` statements are not supported in handler bodies yet"),
            "a handler body holds assignments, `if`/`elif`/`else`, `for`, `match`, and `return`; move other work into the body of one of those",
        )
        .located(file, line)),
    }
}

/// Parse `name = <expr>`, resolving the local's type from the value.
fn parse_assignment(
    assignment: &Node<'_>,
    source: &str,
    file: &str,
    env: &mut HashMap<String, TypeRef>,
) -> Result<Stmt, Diagnostic> {
    let line = line_of(assignment);
    let target = assignment.child_by_field_name("left").ok_or_else(|| {
        Diagnostic::blocker(
            "E1007",
            "assignment without a left-hand side",
            "assign to a local name, for example `total = 1`",
        )
        .located(file, line)
    })?;
    if target.kind() != "identifier" {
        return Err(Diagnostic::blocker(
            "E1007",
            format!(
                "`{}` is not a name this subset can assign to; use a plain local name",
                node_text(&target, source).trim()
            ),
            "assign to a local name, not to a field, a subscript, or a tuple",
        )
        .located(file, line));
    }
    let name = node_text(&target, source);
    if !crate::parser::is_safe_identifier(name) {
        return Err(Diagnostic::blocker(
            "E1011",
            format!("local name `{name}` is not a safe Rust identifier"),
            "rename the local to a snake_case Rust-safe identifier",
        )
        .located(file, line));
    }
    let value_node = assignment.child_by_field_name("right").ok_or_else(|| {
        Diagnostic::blocker(
            "E1007",
            format!("assignment to `{name}` has no value"),
            "give the assignment a value, for example `total = 1`",
        )
        .located(file, line)
    })?;
    let value = translate(&value_node, source, file, line)?;
    let ty = infer::infer(&value, env).map_err(|error| {
        Diagnostic::blocker(
            "E1010",
            format!("cannot resolve the type of `{name}`: {}", error.message()),
            "assign a value whose type the front end can resolve, or declare the value as a handler parameter",
        )
        .located(file, line)
    })?;
    // A local keeps one type. Python would let `total` become a string here,
    // but the generated crate binds it once and would fail to compile, so
    // this is a diagnostic rather than a cargo error.
    if let Some(bound) = env.get(name)
        && *bound != ty
    {
        return Err(Diagnostic::blocker(
            "E1016",
            format!(
                "`{name}` already holds a `{}`; this assignment gives it a `{}`",
                bound.label(),
                ty.label()
            ),
            format!(
                "assign a `{}` value, or bind the new value under a different name",
                bound.label()
            ),
        )
        .located(file, line));
    }
    env.insert(name.to_string(), ty.clone());
    Ok(Stmt::Assign {
        name: name.to_string(),
        ty,
        value,
    })
}

/// Parse `if`/`elif`/`else`.
fn parse_if(
    statement: &Node<'_>,
    source: &str,
    file: &str,
    env: &mut HashMap<String, TypeRef>,
) -> Result<Stmt, Diagnostic> {
    let line = line_of(statement);
    let condition = statement.child_by_field_name("condition").ok_or_else(|| {
        Diagnostic::blocker(
            "E1007",
            "`if` without a condition",
            "write a condition, for example `if total > 100:`",
        )
        .located(file, line)
    })?;
    let consequence = statement
        .child_by_field_name("consequence")
        .ok_or_else(|| {
            Diagnostic::blocker(
                "E1007",
                "`if` without a body",
                "give the branch a body that returns a value",
            )
            .located(file, line)
        })?;

    let mut branches = Vec::new();
    branches.push((
        condition_of(&condition, source, file, env, line)?,
        parse_block(&consequence, source, file, env)?,
    ));

    // Every `elif` and `else` is an `alternative`; the field accessor answers
    // only the first, so read the clause children instead.
    let mut otherwise = Vec::new();
    for clause in statement.named_children_all() {
        match clause.kind() {
            "elif_clause" => {
                let clause_line = line_of(&clause);
                let condition = clause.child_by_field_name("condition").ok_or_else(|| {
                    Diagnostic::blocker(
                        "E1007",
                        "`elif` without a condition",
                        "write a condition, for example `elif total > 10:`",
                    )
                    .located(file, clause_line)
                })?;
                let consequence = clause.child_by_field_name("consequence").ok_or_else(|| {
                    Diagnostic::blocker(
                        "E1007",
                        "`elif` without a body",
                        "give the branch a body that returns a value",
                    )
                    .located(file, clause_line)
                })?;
                branches.push((
                    condition_of(&condition, source, file, env, clause_line)?,
                    parse_block(&consequence, source, file, env)?,
                ));
            }
            "else_clause" => {
                let body = clause.child_by_field_name("body").ok_or_else(|| {
                    Diagnostic::blocker(
                        "E1007",
                        "`else` without a body",
                        "give the branch a body that returns a value",
                    )
                    .located(file, line_of(&clause))
                })?;
                otherwise = parse_block(&body, source, file, env)?;
            }
            _ => {}
        }
    }

    Ok(Stmt::If {
        branches,
        otherwise,
    })
}

/// Translate a branch condition and require it to be a `bool`.
///
/// Python would accept any value, but the generated `if` needs a condition,
/// and accepting a truthy integer would translate a value the user meant as a
/// number into a yes/no test.
fn condition_of(
    condition: &Node<'_>,
    source: &str,
    file: &str,
    env: &HashMap<String, TypeRef>,
    line: usize,
) -> Result<Expr, Diagnostic> {
    let text = node_text(condition, source).trim();
    let translated = translate(condition, source, file, line)?;
    let ty = infer::infer(&translated, env).map_err(|error| {
        Diagnostic::blocker(
            "E1010",
            format!("cannot resolve the condition `{text}`: {}", error.message()),
            "compare two values, or use a parameter or local that is a `bool`",
        )
        .located(file, line)
    })?;
    if ty != TypeRef::Bool {
        return Err(Diagnostic::blocker(
            "E1010",
            format!(
                "the condition `{text}` has type `{}`; an `if` needs a `bool`",
                ty.label()
            ),
            "compare the value, for example `if total > 100:`, or declare the parameter as `bool`",
        )
        .located(file, line));
    }
    Ok(translated)
}

/// Translate an expression, anchoring the diagnostic to the real file.
fn translate(node: &Node<'_>, source: &str, file: &str, line: usize) -> Result<Expr, Diagnostic> {
    expr::translate(node, source).map_err(|diag| attach(diag, file, line))
}

/// The statement kind a body cannot carry yet.
fn unsupported(statement: &Node<'_>, source: &str, file: &str, line: usize) -> Diagnostic {
    let text = node_text(statement, source).trim();
    let shown = if text.len() > 40 {
        format!("{}...", &text[..40])
    } else {
        text.to_string()
    };
    Diagnostic::blocker(
        "E1006",
        format!("the statement `{shown}` is not supported in handler bodies yet"),
        "a handler body holds assignments, `if`/`elif`/`else`, `for`, `match`, and `return`; use a local to name an intermediate value",
    )
    .located(file, line)
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
