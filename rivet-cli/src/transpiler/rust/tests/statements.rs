//! Statement rendering: a `for` loop, a `match`, and the clone a parameter
//! needs when a nested body reads it.

use super::*;

#[test]
fn renders_a_for_loop_over_a_list_literal() {
    let blueprint = route_blueprint(
        "sum",
        "/sum",
        vec![],
        ResponseSpec::Json(TypeRef::Int),
        vec![
            Stmt::Assign {
                name: "total".to_string(),
                ty: TypeRef::Int,
                value: Expr::Int(0),
            },
            Stmt::For {
                name: "line".to_string(),
                ty: TypeRef::Int,
                iterable: Expr::Array(vec![Expr::Int(1), Expr::Int(2), Expr::Int(3)]),
                body: vec![Stmt::Assign {
                    name: "total".to_string(),
                    ty: TypeRef::Int,
                    value: Expr::Binary {
                        op: BinOp::Add,
                        left: Box::new(Expr::Ident("total".to_string())),
                        right: Box::new(Expr::Ident("line".to_string())),
                    },
                }],
            },
            Stmt::Return(Expr::Ident("total".to_string())),
        ],
    );

    let project =
        generate_project(&blueprint, &RivetConfig::default(), Path::new(".")).expect("generate");

    assert!(
        project.main_rs.contains("for line in [1i64, 2i64, 3i64] {"),
        "the list literal iterates as an array literal:\n{}",
        project.main_rs
    );
    assert!(
        project.main_rs.contains("let mut total: i64 = 0i64;"),
        "the reassigned binding is declared mutable:\n{}",
        project.main_rs
    );
    assert!(
        project.main_rs.contains("total = total + line;"),
        "the loop reassigns the outer binding:\n{}",
        project.main_rs
    );
}

#[test]
fn renders_a_match_over_an_integer() {
    let blueprint = route_blueprint(
        "grade",
        "/grade/{score}",
        vec![RouteParam {
            name: "score".to_string(),
            ty: TypeRef::Int,
        }],
        ResponseSpec::Json(TypeRef::String),
        vec![Stmt::Match {
            subject: Expr::Ident("score".to_string()),
            arms: vec![
                (
                    Some(Expr::Int(1)),
                    vec![Stmt::Return(Expr::Str("low".to_string()))],
                ),
                (None, vec![Stmt::Return(Expr::Str("high".to_string()))]),
            ],
        }],
    );

    let project =
        generate_project(&blueprint, &RivetConfig::default(), Path::new(".")).expect("generate");

    assert!(
        project.main_rs.contains("match score {"),
        "a bare identifier needs no parentheses:\n{}",
        project.main_rs
    );
    assert!(
        project.main_rs.contains("1i64 => {"),
        "an integer pattern carries its suffix:\n{}",
        project.main_rs
    );
    assert!(
        project.main_rs.contains("_ => {"),
        "the wildcard arm renders as `_`:\n{}",
        project.main_rs
    );
}

#[test]
fn renders_a_match_over_a_string() {
    let blueprint = route_blueprint(
        "label",
        "/label/{code}",
        vec![RouteParam {
            name: "code".to_string(),
            ty: TypeRef::String,
        }],
        ResponseSpec::Json(TypeRef::String),
        vec![Stmt::Match {
            subject: Expr::Ident("code".to_string()),
            arms: vec![
                (
                    Some(Expr::Str("low".to_string())),
                    vec![Stmt::Return(Expr::Str("small".to_string()))],
                ),
                (None, vec![Stmt::Return(Expr::Str("big".to_string()))]),
            ],
        }],
    );

    let project =
        generate_project(&blueprint, &RivetConfig::default(), Path::new(".")).expect("generate");

    assert!(
        project.main_rs.contains("match code.as_str() {"),
        "a string subject matches through `as_str`:\n{}",
        project.main_rs
    );
    assert!(
        project.main_rs.contains("\"low\" => {"),
        "a string pattern renders as a string literal:\n{}",
        project.main_rs
    );
}

#[test]
fn a_parameter_read_inside_a_match_arm_is_cloned() {
    let blueprint = route_blueprint(
        "f",
        "/f/{name}/{code}",
        vec![
            RouteParam {
                name: "name".to_string(),
                ty: TypeRef::String,
            },
            RouteParam {
                name: "code".to_string(),
                ty: TypeRef::String,
            },
        ],
        ResponseSpec::Json(TypeRef::String),
        vec![
            Stmt::Assign {
                name: "outside".to_string(),
                ty: TypeRef::String,
                value: Expr::Ident("name".to_string()),
            },
            Stmt::Match {
                subject: Expr::Ident("code".to_string()),
                arms: vec![
                    (
                        Some(Expr::Str("a".to_string())),
                        vec![Stmt::For {
                            name: "item".to_string(),
                            ty: TypeRef::Int,
                            iterable: Expr::Array(vec![Expr::Int(1)]),
                            body: vec![Stmt::Return(Expr::Ident("name".to_string()))],
                        }],
                    ),
                    (None, vec![Stmt::Return(Expr::Ident("outside".to_string()))]),
                ],
            },
        ],
    );

    let project =
        generate_project(&blueprint, &RivetConfig::default(), Path::new(".")).expect("generate");

    // `name` is read once here and once inside a `for` body nested in a match
    // arm, so the identifier count is two and the owned String clones here.
    assert!(
        project
            .main_rs
            .contains("let outside: String = name.clone();"),
        "a parameter read in an arm and outside it clones:\n{}",
        project.main_rs
    );
}

/// A `dict` route may fall off its end, which Python answers as `None`. The
/// generated function needs the JSON null tail, or it ends in `()` against a
/// `serde_json::Value` return type and fails with `E0308`.
///
/// A `for` may run zero times, so it never completes a handler. Before this
/// tail existed, a body whose last statement was a `for` produced a function
/// that did not compile.
#[test]
fn a_body_that_ends_in_a_loop_answers_json_null() {
    let blueprint = route_blueprint(
        "sum_tail",
        "/sum/{id}",
        vec![RouteParam {
            name: "id".to_string(),
            ty: TypeRef::Int,
        }],
        ResponseSpec::Json(TypeRef::Json),
        vec![
            Stmt::Assign {
                name: "total".to_string(),
                ty: TypeRef::Int,
                value: Expr::Int(0),
            },
            Stmt::For {
                name: "line".to_string(),
                ty: TypeRef::Int,
                iterable: Expr::Array(vec![Expr::Int(1), Expr::Int(2), Expr::Int(3)]),
                body: vec![Stmt::Assign {
                    name: "total".to_string(),
                    ty: TypeRef::Int,
                    value: Expr::Binary {
                        op: BinOp::Add,
                        left: Box::new(Expr::Ident("total".to_string())),
                        right: Box::new(Expr::Ident("line".to_string())),
                    },
                }],
            },
        ],
    );

    let project =
        generate_project(&blueprint, &RivetConfig::default(), Path::new(".")).expect("generate");

    assert!(
        project.main_rs.contains("for line in [1i64, 2i64, 3i64] {"),
        "the loop is rendered:\n{}",
        project.main_rs
    );
    assert!(
        project.main_rs.contains("serde_json::Value::Null\n    }\n"),
        "a body that can fall through ends with the JSON null tail:\n{}",
        project.main_rs
    );
}

/// A body that returns on every path must NOT gain the tail: it already ends
/// in a diverging expression, so a second expression would not compile.
#[test]
fn a_body_that_returns_on_every_path_has_no_null_tail() {
    let blueprint = route_blueprint(
        "grade",
        "/grade/{score}",
        vec![RouteParam {
            name: "score".to_string(),
            ty: TypeRef::Int,
        }],
        ResponseSpec::Json(TypeRef::Json),
        vec![Stmt::Match {
            subject: Expr::Ident("score".to_string()),
            arms: vec![
                (
                    Some(Expr::Int(1)),
                    vec![Stmt::Return(Expr::Object(vec![(
                        "grade".to_string(),
                        Expr::Str("low".to_string()),
                    )]))],
                ),
                (
                    None,
                    vec![Stmt::Return(Expr::Object(vec![(
                        "grade".to_string(),
                        Expr::Str("high".to_string()),
                    )]))],
                ),
            ],
        }],
    );

    let project =
        generate_project(&blueprint, &RivetConfig::default(), Path::new(".")).expect("generate");

    assert!(
        !project.main_rs.contains("serde_json::Value::Null\n    }\n"),
        "a completing body takes no tail:\n{}",
        project.main_rs
    );
}
