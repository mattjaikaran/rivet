//! The expression and statement language of a handler body.
//!
//! Kept beside the route types rather than inside them: a route *names* a
//! body, while this module is the body's own vocabulary.

use super::TypeRef;
use serde::{Deserialize, Serialize};

/// A binary operation inside a handler expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BinOp {
    /// `+`; also concatenates two strings.
    Add,
    Sub,
    Mul,
    /// `/`, which is always true division in Python and therefore yields a
    /// float even when both operands are integers.
    ///
    /// `//` and `%` are absent on purpose. Python floors toward negative
    /// infinity and gives the result the divisor's sign; Rust truncates and
    /// gives it the dividend's, so `-7 // 2` and `-7 % 3` differ between the
    /// two languages. Rendering either would translate one statement into two
    /// different answers, so the front end rejects both instead of choosing.
    Div,
    /// Comparisons, which yield a `bool`.
    Eq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    /// `and`, which short-circuits.
    And,
    /// `or`, which short-circuits.
    Or,
}

impl BinOp {
    /// Whether the operator yields a `bool`.
    pub fn is_comparison(self) -> bool {
        matches!(
            self,
            BinOp::Eq | BinOp::NotEq | BinOp::Lt | BinOp::LtEq | BinOp::Gt | BinOp::GtEq
        )
    }

    /// The operator as the DSL writes it, for diagnostics.
    pub fn as_str(self) -> &'static str {
        match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Eq => "==",
            BinOp::NotEq => "!=",
            BinOp::Lt => "<",
            BinOp::LtEq => "<=",
            BinOp::Gt => ">",
            BinOp::GtEq => ">=",
            BinOp::And => "and",
            BinOp::Or => "or",
        }
    }
}

/// The type a binary operation yields, or `None` when the operand types do
/// not combine.
///
/// This is the one place the rule lives, so the parser that resolves a
/// local's type and the generator that renders it cannot disagree.
///
/// The rule is Python's: `/` always yields a float, `//`, `+`, `-`, `*`, and
/// `%` yield an integer for integer operands and a float when either operand
/// is a float, `+` concatenates two strings, a comparison yields a bool, and
/// `and`/`or` take and yield a bool. Booleans do not take part in arithmetic,
/// though Python would treat them as 0 and 1: agreeing with that would
/// translate `total` and `count` the same way, so the DSL refuses instead.
pub fn binary_result(op: BinOp, left: &TypeRef, right: &TypeRef) -> Option<TypeRef> {
    use TypeRef::{Bool, Float, Int, String};
    if op.is_comparison() {
        // Every comparison the DSL accepts is between two numbers or two
        // strings, and both spellings yield a bool.
        let comparable = matches!(
            (left, right),
            (Int, Int) | (Float, Float) | (Int, Float) | (Float, Int) | (String, String)
        );
        return comparable.then_some(Bool);
    }
    match op {
        BinOp::And | BinOp::Or => (left == &Bool && right == &Bool).then_some(Bool),
        BinOp::Add if left == &String && right == &String => Some(String),
        BinOp::Div => {
            let numeric = matches!(
                (left, right),
                (Int, Int) | (Float, Float) | (Int, Float) | (Float, Int)
            );
            numeric.then_some(Float)
        }
        BinOp::Add | BinOp::Sub | BinOp::Mul => match (left, right) {
            (Int, Int) => Some(Int),
            (Float, Float) | (Int, Float) | (Float, Int) => Some(Float),
            _ => None,
        },
        BinOp::Eq | BinOp::NotEq | BinOp::Lt | BinOp::LtEq | BinOp::Gt | BinOp::GtEq => None,
    }
}

/// A handler return expression, translated from the DSL.
///
/// Values are stored typed so generators can render each one into the target
/// language without re-analyzing the source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Expr {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    /// `[a, b]` — an array literal.
    Array(Vec<Expr>),
    /// `{"key": value}` — an object literal with string keys.
    Object(Vec<(String, Expr)>),
    /// A reference to a handler parameter or a local the body assigned.
    Ident(String),
    /// A DTO constructor call such as `OrderResponse(status="ok")`.
    Construct {
        ty: String,
        args: Vec<(String, Expr)>,
    },
    /// `a + b`, `a < b`, `a and b` — an operation on two operands.
    ///
    /// A chained Python comparison such as `a < b < c` never reaches the IR:
    /// the parser lowers it to `a < b and b < c`, because the chain means the
    /// same as that conjunction and not what nesting the operators would.
    Binary {
        op: BinOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    /// `not a`.
    Not(Box<Expr>),
}

/// One statement of a handler body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Stmt {
    /// `return <expr>`. A bare `return` lowers to [`Expr::Null`].
    Return(Expr),
    /// `name = <expr>`.
    ///
    /// The parser resolves `ty` from `value`, so the generator renders the
    /// binding without a second type analysis.
    Assign {
        name: String,
        ty: TypeRef,
        value: Expr,
    },
    /// `if`/`elif`/`else`.
    ///
    /// Each entry of `branches` is a condition and the body it guards, in
    /// source order. `otherwise` is the `else` body, empty when the statement
    /// carries no `else`.
    If {
        branches: Vec<(Expr, Vec<Stmt>)>,
        otherwise: Vec<Stmt>,
    },
}

impl Stmt {
    /// Whether every path through `body` ends in a `return`.
    ///
    /// A handler whose response carries a value must return on every path, so
    /// the parser asks this before it accepts the body. An `if` completes
    /// only when it has an `else` and every branch completes.
    pub fn all_paths_return(body: &[Stmt]) -> bool {
        match body.last() {
            Some(Stmt::Return(_)) => true,
            Some(Stmt::If {
                branches,
                otherwise,
            }) => {
                !otherwise.is_empty()
                    && Self::all_paths_return(otherwise)
                    && branches
                        .iter()
                        .all(|(_, branch)| Self::all_paths_return(branch))
            }
            _ => false,
        }
    }

    /// Every statement of `body`, in source order, including nested blocks.
    pub fn walk(body: &[Stmt]) -> Vec<&Stmt> {
        let mut out = Vec::new();
        for stmt in body {
            out.push(stmt);
            if let Stmt::If {
                branches,
                otherwise,
            } = stmt
            {
                for (_, branch) in branches {
                    out.extend(Self::walk(branch));
                }
                out.extend(Self::walk(otherwise));
            }
        }
        out
    }
}
