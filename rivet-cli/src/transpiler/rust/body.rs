//! Handler-body rendering: one IR [`Stmt`] into Rust statements.
//!
//! The renderer emits at column zero and pads each line itself, so the service
//! layer indents a whole block in one step. A block owns the bindings it
//! declares, so `env` and `declared` are cloned for every nested body: a name
//! bound inside one branch is not in scope in a sibling branch, in a loop body,
//! or in a statement below the branch.

use super::{Codegen, Emitter, rust_str};
use crate::diagnostic::Diagnostic;
use rivet_core::infer::infer;
use rivet_core::ir::{Expr, ResponseSpec, Stmt, TypeRef};
use std::collections::{HashMap, HashSet};

/// The context a body render carries down its recursion.
///
/// It is the same at every level — the surrounding function supplies it once —
/// so it travels as one value rather than as four arguments.
pub(super) struct BodyContext<'a, 'c> {
    pub(super) codegen: &'a Codegen<'c>,
    pub(super) counts: &'a HashMap<String, usize>,
    pub(super) response: &'a ResponseSpec,
}

/// Render a handler body as Rust statements, at column zero.
///
/// Each statement is one line (an `if` block spans several), and `render_fn`
/// indents the whole block. The environment gains a binding for every local
/// the walk assigns, so a later statement resolves the local's type. The
/// `declared` set records which names this block emitted a `let` for, so a
/// later assignment to one becomes a plain reassignment.
pub(super) fn render_body(
    context: &BodyContext<'_, '_>,
    body: &[Stmt],
    env: &mut HashMap<String, TypeRef>,
    declared: &mut HashSet<String>,
    indent: usize,
) -> Result<String, Diagnostic> {
    let pad = "    ".repeat(indent);
    let mut out = String::new();
    for (i, stmt) in body.iter().enumerate() {
        match stmt {
            Stmt::Assign { name, ty, value } => {
                let rendered = {
                    let emitter = Emitter {
                        codegen: context.codegen,
                        params: env,
                        counts: context.counts,
                    };
                    emitter.render_typed(value, ty)?
                };
                if declared.contains(name) {
                    // The name is already declared in this block, so this
                    // site reassigns the existing binding.
                    out.push_str(&format!("{pad}{name} = {rendered};\n"));
                } else {
                    let rust_ty = context.codegen.rust_type(ty, false)?;
                    let mut_kw = if assigned_later(&body[i + 1..], name) {
                        "mut "
                    } else {
                        ""
                    };
                    out.push_str(&format!(
                        "{pad}let {mut_kw}{name}: {rust_ty} = {rendered};\n"
                    ));
                    env.insert(name.clone(), ty.clone());
                    declared.insert(name.clone());
                }
            }
            Stmt::Return(expr) => {
                if matches!(context.response, ResponseSpec::None) {
                    // A bare `return` lowers to `Expr::Null`; under `-> None`
                    // it renders as the plain `return;`.
                    out.push_str(&format!("{pad}return;\n"));
                } else {
                    let value = {
                        let emitter = Emitter {
                            codegen: context.codegen,
                            params: env,
                            counts: context.counts,
                        };
                        emitter.render_return_value(expr, context.response)?
                    };
                    out.push_str(&format!("{pad}return {value};\n"));
                }
            }
            Stmt::If {
                branches,
                otherwise,
            } => {
                // Render every condition first, then each branch against a
                // clone of the environment, so a binding made in one branch
                // never leaks into a sibling branch or the statements after
                // the `if`.
                let mut conditions = Vec::with_capacity(branches.len());
                for (cond, _) in branches {
                    let emitter = Emitter {
                        codegen: context.codegen,
                        params: env,
                        counts: context.counts,
                    };
                    conditions.push(emitter.render_typed(cond, &TypeRef::Bool)?);
                }
                for (i, ((_, branch), cond)) in branches.iter().zip(&conditions).enumerate() {
                    if i == 0 {
                        out.push_str(&format!("{pad}if {cond} {{\n"));
                    } else {
                        out.push_str(&format!("{pad}}} else if {cond} {{\n"));
                    }
                    let mut inner = env.clone();
                    let mut inner_declared = declared.clone();
                    out.push_str(&render_body(
                        context,
                        branch,
                        &mut inner,
                        &mut inner_declared,
                        indent + 1,
                    )?);
                }
                if !otherwise.is_empty() {
                    out.push_str(&format!("{pad}}} else {{\n"));
                    let mut inner = env.clone();
                    let mut inner_declared = declared.clone();
                    out.push_str(&render_body(
                        context,
                        otherwise,
                        &mut inner,
                        &mut inner_declared,
                        indent + 1,
                    )?);
                }
                out.push_str(&format!("{pad}}}\n"));
            }
            Stmt::For {
                name,
                ty,
                iterable,
                body,
            } => {
                let rendered = {
                    let emitter = Emitter {
                        codegen: context.codegen,
                        params: env,
                        counts: context.counts,
                    };
                    emitter.render_iterable(iterable, ty)?
                };
                out.push_str(&format!("{pad}for {name} in {rendered} {{\n"));
                let mut inner = env.clone();
                let mut inner_declared = declared.clone();
                inner.insert(name.clone(), ty.clone());
                out.push_str(&render_body(
                    context,
                    body,
                    &mut inner,
                    &mut inner_declared,
                    indent + 1,
                )?);
                out.push_str(&format!("{pad}}}\n"));
            }
            Stmt::Match { subject, arms } => {
                let emitter = Emitter {
                    codegen: context.codegen,
                    params: env,
                    counts: context.counts,
                };
                let ty = infer(subject, env).map_err(|err| {
                    Diagnostic::blocker(
                        "E2002",
                        err.message(),
                        "fix the handler body so its expressions have types that combine",
                    )
                    .located("<generated>", 1)
                })?;
                let mut scrutinee = emitter.render_typed(subject, &ty)?;
                if ty == TypeRef::String {
                    scrutinee = format!("{scrutinee}.as_str()");
                }
                if matches!(subject, Expr::Binary { .. } | Expr::Not(_)) {
                    scrutinee = format!("({scrutinee})");
                }
                out.push_str(&format!("{pad}match {scrutinee} {{\n"));
                for (pattern, arm) in arms {
                    let pattern = match pattern {
                        None => "_".to_string(),
                        Some(expr) => render_pattern(expr)?,
                    };
                    out.push_str(&format!("{pad}    {pattern} => {{\n"));
                    let mut inner = env.clone();
                    let mut inner_declared = declared.clone();
                    out.push_str(&render_body(
                        context,
                        arm,
                        &mut inner,
                        &mut inner_declared,
                        indent + 2,
                    )?);
                    out.push_str(&format!("{pad}    }}\n"));
                }
                out.push_str(&format!("{pad}}}\n"));
            }
        }
    }
    Ok(out)
}

/// Whether a later statement of the same block assigns `name`.
///
/// The walk descends into nested blocks, so a later `for`/`if`/`match` whose
/// body assigns `name` also counts. A same-name assignment in a sibling block
/// does not, because the slice is this block's own tail.
fn assigned_later(body: &[Stmt], name: &str) -> bool {
    Stmt::walk(body)
        .iter()
        .any(|stmt| matches!(stmt, Stmt::Assign { name: assigned, .. } if assigned == name))
}

/// Render one `case` pattern as a Rust match pattern.
///
/// Every pattern the parser accepts is a literal of the subject's type, and
/// the wildcard arm carries no [`Expr`]. Any other shape is a parser/generator
/// disagreement, so it is a blocker rather than a wrong pattern.
fn render_pattern(expr: &Expr) -> Result<String, Diagnostic> {
    match expr {
        Expr::Int(value) => Ok(format!("{value}i64")),
        Expr::Float(value) => Ok(format!("{value:?}f64")),
        Expr::Str(value) => Ok(rust_str(value)),
        Expr::Bool(value) => Ok(value.to_string()),
        // No `Expr::Null` arm: the parser refuses a `dict` subject, so no
        // pattern can reach here without a scalar subject type.
        _ => Err(Diagnostic::blocker(
            "E2002",
            "a `case` pattern must be a literal of the subject's type",
            "write a literal pattern, or use `case _` to match every other value",
        )
        .located("<generated>", 1)),
    }
}
