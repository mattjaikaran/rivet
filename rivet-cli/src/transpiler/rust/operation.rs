//! Rendering of binary and unary (`not`) operations.
//!
//! These render one operation into Rust, reading operand types through
//! [`infer`] and the result type through [`binary_result`], so the generator
//! never re-implements the one type rule. They live apart from the expression
//! renderer because they are the second half of handler-body rendering.

use super::Emitter;
use crate::diagnostic::Diagnostic;
use rivet_core::infer::{InferError, infer};
use rivet_core::ir::{BinOp, Expr, TypeRef, binary_result};

impl Emitter<'_> {
    /// Render a binary operation at its own inferred type.
    pub(super) fn render_binary(
        &self,
        op: BinOp,
        left: &Expr,
        right: &Expr,
    ) -> Result<String, Diagnostic> {
        let left_ty = infer(left, self.params).map_err(|err| self.infer_error(err))?;
        let right_ty = infer(right, self.params).map_err(|err| self.infer_error(err))?;
        match op {
            BinOp::And | BinOp::Or => {
                let operator = if op == BinOp::And { "&&" } else { "||" };
                let l = self.render_operand(left, &TypeRef::Bool)?;
                let r = self.render_operand(right, &TypeRef::Bool)?;
                Ok(format!("{l} {operator} {r}"))
            }
            // Python's `/` is true division, so both operands render in a
            // float context even when they are integers.
            BinOp::Div => {
                let l = self.render_operand(left, &TypeRef::Float)?;
                let r = self.render_operand(right, &TypeRef::Float)?;
                Ok(format!("{l} / {r}"))
            }
            // `+` on two strings concatenates without moving either operand.
            BinOp::Add if left_ty == TypeRef::String && right_ty == TypeRef::String => {
                let l = self.render_typed(left, &TypeRef::String)?;
                let r = self.render_typed(right, &TypeRef::String)?;
                Ok(format!("format!(\"{{}}{{}}\", {l}, {r})"))
            }
            BinOp::Add | BinOp::Sub | BinOp::Mul => {
                let result_ty = binary_result(op, &left_ty, &right_ty).ok_or_else(|| {
                    self.infer_error(InferError::Operands {
                        op,
                        left: Box::new(left_ty.clone()),
                        right: Box::new(right_ty.clone()),
                    })
                })?;
                let l = self.render_operand(left, &result_ty)?;
                let r = self.render_operand(right, &result_ty)?;
                Ok(format!("{l} {} {r}", op.as_str()))
            }
            // A mixed int/float comparison promotes both operands to f64;
            // Rust will not compare an `i64` with an `f64`.
            BinOp::Eq | BinOp::NotEq | BinOp::Lt | BinOp::LtEq | BinOp::Gt | BinOp::GtEq => {
                let mixed = matches!(
                    (&left_ty, &right_ty),
                    (TypeRef::Int, TypeRef::Float) | (TypeRef::Float, TypeRef::Int)
                );
                if mixed {
                    let l = self.render_operand(left, &TypeRef::Float)?;
                    let r = self.render_operand(right, &TypeRef::Float)?;
                    Ok(format!("{l} {} {r}", op.as_str()))
                } else {
                    let l = self.render_operand(left, &left_ty)?;
                    let r = self.render_operand(right, &right_ty)?;
                    Ok(format!("{l} {} {r}", op.as_str()))
                }
            }
        }
    }

    /// Render `not a`, which negates a boolean expression.
    pub(super) fn render_not(&self, operand: &Expr) -> Result<String, Diagnostic> {
        let rendered = self.render_typed(operand, &TypeRef::Bool)?;
        Ok(format!("!({rendered})"))
    }

    /// Coerce a rendered operation to its target type. The only legal
    /// coercion is an integer operation into a float, which `/` and a mixed
    /// int/float comparison ask for; anything else means the generator and
    /// the parser disagree.
    pub(super) fn coerce_operation(
        &self,
        rendered: String,
        expr: &Expr,
        ty: &TypeRef,
    ) -> Result<String, Diagnostic> {
        let inferred = infer(expr, self.params).map_err(|err| self.infer_error(err))?;
        if inferred == *ty {
            Ok(rendered)
        } else if inferred == TypeRef::Int && *ty == TypeRef::Float {
            Ok(format!("({rendered}) as f64"))
        } else {
            Err(self.type_error(expr, ty))
        }
    }

    /// Render an operand of an infix operator, parenthesizing a nested
    /// operation so the emitted expression keeps the source's grouping.
    fn render_operand(&self, expr: &Expr, ty: &TypeRef) -> Result<String, Diagnostic> {
        let rendered = self.render_typed(expr, ty)?;
        if matches!(expr, Expr::Binary { .. } | Expr::Not(_)) {
            Ok(format!("({rendered})"))
        } else {
            Ok(rendered)
        }
    }

    /// Wrap an inference failure into a diagnostic. The parser validates every
    /// body, so a failure here means the generator and the parser disagree.
    fn infer_error(&self, err: InferError) -> Diagnostic {
        Diagnostic::blocker(
            "E2002",
            err.message(),
            "fix the handler body so its expressions have types that combine",
        )
        .located("<generated>", 1)
    }
}
