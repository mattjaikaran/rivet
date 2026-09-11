//! Expression type inference.
//!
//! The parser resolves a local's type when it binds one, and the generator
//! reads that type back. Both need the same rule, so the rule lives here
//! rather than in either of them: two copies would eventually disagree, and a
//! disagreement shows up as a generated crate that does not compile.
//!
//! The environment maps a name to its type. It holds the handler's parameters
//! and every local the body has assigned so far.

use crate::ir::{BinOp, Expr, TypeRef};
use std::collections::HashMap;

/// Why an expression has no type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InferError {
    /// The name is neither a parameter nor a local.
    Unknown(String),
    /// The operand types do not combine under the operator.
    Operands {
        op: BinOp,
        left: Box<TypeRef>,
        right: Box<TypeRef>,
    },
    /// The operand is not the bool this operator needs.
    NotABool(Box<TypeRef>),
    /// The list is empty, so its element type is unknown.
    EmptyList,
    /// The list's items do not all have the same type.
    MixedList,
}

impl InferError {
    /// A message naming the expression, for a diagnostic.
    pub fn message(&self) -> String {
        match self {
            InferError::Unknown(name) => {
                format!("`{name}` is not a parameter or a local of this handler")
            }
            InferError::Operands { op, left, right } => format!(
                "`{}` cannot combine `{}` and `{}`",
                op.as_str(),
                left.label(),
                right.label()
            ),
            InferError::NotABool(ty) => {
                format!("a boolean operation needs a `bool`, not `{}`", ty.label())
            }
            InferError::EmptyList => {
                "an empty list literal has no element type; declare a parameter or local of the element type instead".to_string()
            }
            InferError::MixedList => {
                "a list literal must hold values of one type".to_string()
            }
        }
    }
}

/// The type of `expr`, resolved against `env`.
pub fn infer(expr: &Expr, env: &HashMap<String, TypeRef>) -> Result<TypeRef, InferError> {
    match expr {
        Expr::Null => Ok(TypeRef::Json),
        Expr::Bool(_) => Ok(TypeRef::Bool),
        Expr::Int(_) => Ok(TypeRef::Int),
        Expr::Float(_) => Ok(TypeRef::Float),
        Expr::Str(_) => Ok(TypeRef::String),
        Expr::Ident(name) => env
            .get(name)
            .cloned()
            .ok_or_else(|| InferError::Unknown(name.clone())),
        // A DTO constructor names its own type, so the environment is not
        // needed and the blueprint check happens elsewhere.
        Expr::Construct { ty, .. } => Ok(TypeRef::Named(ty.clone())),
        Expr::Object(_) => Ok(TypeRef::Json),
        Expr::Array(items) => {
            let Some(first) = items.first() else {
                return Err(InferError::EmptyList);
            };
            let element = infer(first, env)?;
            for item in &items[1..] {
                if infer(item, env)? != element {
                    return Err(InferError::MixedList);
                }
            }
            Ok(TypeRef::Array {
                element: Box::new(element),
                len: None,
            })
        }
        Expr::Not(operand) => {
            let ty = infer(operand, env)?;
            if ty == TypeRef::Bool {
                Ok(TypeRef::Bool)
            } else {
                Err(InferError::NotABool(Box::new(ty)))
            }
        }
        Expr::Binary { op, left, right } => {
            let left_ty = infer(left, env)?;
            let right_ty = infer(right, env)?;
            crate::ir::binary_result(*op, &left_ty, &right_ty).ok_or(InferError::Operands {
                op: *op,
                left: Box::new(left_ty),
                right: Box::new(right_ty),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, TypeRef)]) -> HashMap<String, TypeRef> {
        pairs
            .iter()
            .map(|(name, ty)| (name.to_string(), ty.clone()))
            .collect()
    }

    #[test]
    fn a_name_resolves_from_the_environment_or_fails() {
        let types = env(&[("page", TypeRef::Int)]);
        assert_eq!(
            infer(&Expr::Ident("page".to_string()), &types),
            Ok(TypeRef::Int)
        );
        assert_eq!(
            infer(&Expr::Ident("missing".to_string()), &types),
            Err(InferError::Unknown("missing".to_string()))
        );
    }

    /// Python's `/` is true division, so an integer pair yields a float and
    /// the generator must render a cast rather than integer division.
    #[test]
    fn division_infers_a_float_for_integer_operands() {
        let types = env(&[("total", TypeRef::Int)]);
        let expr = Expr::Binary {
            op: BinOp::Div,
            left: Box::new(Expr::Ident("total".to_string())),
            right: Box::new(Expr::Int(2)),
        };
        assert_eq!(infer(&expr, &types), Ok(TypeRef::Float));
    }

    #[test]
    fn mismatched_operands_report_the_operator_and_both_types() {
        let types = env(&[("name", TypeRef::String)]);
        let expr = Expr::Binary {
            op: BinOp::Sub,
            left: Box::new(Expr::Ident("name".to_string())),
            right: Box::new(Expr::Int(1)),
        };
        let error = infer(&expr, &types).expect_err("a string minus an int has no type");
        assert!(
            error.message().contains('-'),
            "the message names the operator: {}",
            error.message()
        );
    }

    #[test]
    fn a_list_takes_its_element_type_from_its_items() {
        let types = env(&[]);
        let expr = Expr::Array(vec![Expr::Int(1), Expr::Int(2)]);
        assert_eq!(
            infer(&expr, &types),
            Ok(TypeRef::Array {
                element: Box::new(TypeRef::Int),
                len: None,
            })
        );
        assert_eq!(
            infer(&Expr::Array(vec![]), &types),
            Err(InferError::EmptyList)
        );
        assert_eq!(
            infer(
                &Expr::Array(vec![Expr::Int(1), Expr::Str("x".into())]),
                &types
            ),
            Err(InferError::MixedList)
        );
    }
}
