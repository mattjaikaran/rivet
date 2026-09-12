//! A `match` subject that is an operation or a `not`: neither needs outer
//! parentheses. Wrapping one trips `unused_parens` in the generated crate,
//! which the warning-free rule forbids.
use super::*;

/// A `match` subject that is an operation or a `not` needs no parentheses.
/// Wrapping one trips `unused_parens` in the generated crate, which the
/// warning-free rule forbids.
#[test]
fn an_operation_match_subject_is_not_parenthesized() {
    let blueprint = route_blueprint(
        "bin",
        "/bin/{qty}",
        vec![RouteParam {
            name: "qty".to_string(),
            ty: TypeRef::Int,
        }],
        ResponseSpec::Json(TypeRef::Json),
        vec![Stmt::Match {
            subject: Expr::Binary {
                op: BinOp::Add,
                left: Box::new(Expr::Ident("qty".to_string())),
                right: Box::new(Expr::Int(1)),
            },
            arms: vec![
                (
                    Some(Expr::Int(2)),
                    vec![Stmt::Return(Expr::Object(vec![(
                        "g".to_string(),
                        Expr::Str("a".to_string()),
                    )]))],
                ),
                (
                    None,
                    vec![Stmt::Return(Expr::Object(vec![(
                        "g".to_string(),
                        Expr::Str("b".to_string()),
                    )]))],
                ),
            ],
        }],
    );

    let project =
        generate_project(&blueprint, &RivetConfig::default(), Path::new(".")).expect("generate");

    assert!(
        project.main_rs.contains("match qty + 1i64 {"),
        "an operation subject is not parenthesized:\n{}",
        project.main_rs
    );
    assert!(
        !project.main_rs.contains("match (qty + 1i64) {"),
        "the parenthesized form trips `unused_parens`:\n{}",
        project.main_rs
    );
}

/// A `not` subject is unparenthesized too. `render_not` supplies its own
/// parentheses around the operand, so the scrutinee needs no outer pair.
#[test]
fn a_not_match_subject_is_not_parenthesized() {
    let blueprint = route_blueprint(
        "neg",
        "/neg/{flag}",
        vec![RouteParam {
            name: "flag".to_string(),
            ty: TypeRef::Bool,
        }],
        ResponseSpec::Json(TypeRef::Json),
        vec![Stmt::Match {
            subject: Expr::Not(Box::new(Expr::Ident("flag".to_string()))),
            arms: vec![
                (
                    Some(Expr::Bool(true)),
                    vec![Stmt::Return(Expr::Object(vec![(
                        "g".to_string(),
                        Expr::Str("t".to_string()),
                    )]))],
                ),
                (
                    None,
                    vec![Stmt::Return(Expr::Object(vec![(
                        "g".to_string(),
                        Expr::Str("f".to_string()),
                    )]))],
                ),
            ],
        }],
    );

    let project =
        generate_project(&blueprint, &RivetConfig::default(), Path::new(".")).expect("generate");

    assert!(
        project.main_rs.contains("match !(flag) {"),
        "a `not` subject carries the operand's own parentheses:\n{}",
        project.main_rs
    );
    assert!(
        !project.main_rs.contains("match (!(flag)) {"),
        "the outer pair trips `unused_parens`:\n{}",
        project.main_rs
    );
}
