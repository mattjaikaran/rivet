//! Statements in a handler body: assignment, `if`/`else`, and several
//! `return`s, plus the operator subset the conditions and values use.

use super::*;
use rivet_core::ir::{BinOp, Stmt};

/// A body may bind a local and return it.
#[test]
fn parses_a_local_binding() {
    let blueprint = parse(
        "from rivet import api\n\n@api.get(\"/ping\")\ndef ping() -> int:\n    total = 7\n    return total\n",
    )
    .expect("parse");
    assert_eq!(
        blueprint.routes[0].body,
        vec![
            Stmt::Assign {
                name: "total".to_string(),
                ty: TypeRef::Int,
                value: Expr::Int(7),
            },
            Stmt::Return(Expr::Ident("total".to_string())),
        ]
    );
}

/// The local's type comes from the value, so a computed local carries the type
/// of the operation rather than the type of its first operand.
#[test]
fn a_local_takes_the_type_of_its_expression() {
    let blueprint = parse(
        "from rivet import api\n\n@api.get(\"/ping\")\ndef ping(qty: int) -> float:\n    half = qty / 2\n    return half\n",
    )
    .expect("parse");
    let Stmt::Assign { ty, .. } = &blueprint.routes[0].body[0] else {
        panic!("the first statement is the binding");
    };
    // Python's `/` is true division, so the local is a float.
    assert_eq!(ty, &TypeRef::Float);
}

/// A branch returns its own value, and the `else` is kept.
#[test]
fn parses_an_if_else_with_a_return_in_each_branch() {
    let source = r#"
from rivet import api

@api.get("/orders", stories=["US-1"])
def orders(qty: int) -> dict:
    if qty > 100:
        return {"size": "bulk"}
    else:
        return {"size": "small"}
"#;
    let blueprint = parse(source).expect("parse");
    let Stmt::If {
        branches,
        otherwise,
    } = &blueprint.routes[0].body[0]
    else {
        panic!("the first statement is the branch");
    };
    assert_eq!(branches.len(), 1);
    assert!(matches!(branches[0].0, Expr::Binary { op: BinOp::Gt, .. }));
    assert_eq!(otherwise.len(), 1, "the else branch is kept");
}

/// `elif` becomes a second branch, not a nested `if`.
#[test]
fn parses_elif_as_a_second_branch() {
    let source = r#"
from rivet import api

@api.get("/n", stories=["US-1"])
def n(qty: int) -> str:
    if qty > 100:
        return "bulk"
    elif qty > 10:
        return "medium"
    else:
        return "small"
"#;
    let blueprint = parse(source).expect("parse");
    let Stmt::If {
        branches,
        otherwise,
    } = &blueprint.routes[0].body[0]
    else {
        panic!("the first statement is the branch");
    };
    assert_eq!(branches.len(), 2, "the elif is its own branch");
    assert_eq!(otherwise.len(), 1);
}

/// A value that carries a concrete type must return on every path, because the
/// generated function has to produce it.
#[test]
fn rejects_a_branching_body_that_can_fall_through() {
    let source = "from rivet import api\n\n@api.get(\"/n\", stories=[\"US-1\"])\ndef n(qty: int) -> int:\n    if qty > 1:\n        return 1\n    return 2\n";
    parse(source).expect("a trailing return completes the body");

    let missing = "from rivet import api\n\n@api.get(\"/n\", stories=[\"US-1\"])\ndef n(qty: int) -> int:\n    if qty > 1:\n        return 1\n";
    let diagnostic = parse(missing).expect_err("a body that can fall off its end must fail");
    assert_eq!(diagnostic.error_code, "E1010");
    assert!(!diagnostic.suggested_fix.is_empty());
}

/// `dict` may fall off its end: Python returns `None` and the route answers
/// JSON null.
#[test]
fn a_dict_body_may_fall_through() {
    let blueprint = parse(
        "from rivet import api\n\n@api.get(\"/n\", stories=[\"US-1\"])\ndef n(qty: int) -> dict:\n    if qty > 1:\n        return {\"n\": qty}\n",
    )
    .expect("a dict body needs no return on every path");
    assert_eq!(blueprint.routes[0].body.len(), 1);
}

/// A condition must be a `bool`. Python would accept a truthy integer, and
/// translating that into a yes/no test would change what the code means.
#[test]
fn rejects_a_non_boolean_condition() {
    let diagnostic = parse(
        "from rivet import api\n\n@api.get(\"/n\", stories=[\"US-1\"])\ndef n(qty: int) -> dict:\n    if qty:\n        return {\"n\": 1}\n    return {\"n\": 0}\n",
    )
    .expect_err("an integer condition must fail");
    assert_eq!(diagnostic.error_code, "E1010");
    assert!(
        diagnostic.message.contains("bool"),
        "the diagnostic asks for a bool: {}",
        diagnostic.message
    );
    assert!(!diagnostic.suggested_fix.is_empty());
}

/// A chained comparison means a conjunction, not a nested comparison.
#[test]
fn a_chained_comparison_becomes_a_conjunction() {
    let blueprint = parse(
        "from rivet import api\n\n@api.get(\"/n\", stories=[\"US-1\"])\ndef n(qty: int) -> dict:\n    if 1 < qty < 10:\n        return {\"in\": True}\n    return {\"in\": False}\n",
    )
    .expect("parse");
    let Stmt::If { branches, .. } = &blueprint.routes[0].body[0] else {
        panic!("the first statement is the branch");
    };
    let Expr::Binary { op, right, .. } = &branches[0].0 else {
        panic!("the condition is an operation");
    };
    assert_eq!(*op, BinOp::And, "the chain is a conjunction");
    assert!(matches!(**right, Expr::Binary { op: BinOp::Lt, .. }));
}

/// `and`, `or`, and `not` are part of the condition language.
#[test]
fn parses_boolean_operators() {
    let source = "from rivet import api\n\n@api.get(\"/n\", stories=[\"US-1\"])\ndef n(a: bool, b: bool) -> dict:\n    if a and not b:\n        return {\"n\": 1}\n    return {\"n\": 0}\n";
    let blueprint = parse(source).expect("parse");
    let Stmt::If { branches, .. } = &blueprint.routes[0].body[0] else {
        panic!("the first statement is the branch");
    };
    assert!(matches!(branches[0].0, Expr::Binary { op: BinOp::And, .. }));
}

/// The operators Rust cannot reproduce on negative operands are rejected
/// rather than translated into a different answer.
#[test]
fn rejects_the_operators_whose_semantics_differ() {
    for operator in ["//", "%"] {
        let source = format!(
            "from rivet import api\n\n@api.get(\"/n\", stories=[\"US-1\"])\ndef n(qty: int) -> int:\n    return qty {operator} 2\n"
        );
        let diagnostic = match parse(&source) {
            Ok(_) => panic!("`{operator}` must be rejected: its meaning differs from Rust's"),
            Err(error) => error,
        };
        assert_eq!(diagnostic.error_code, "E1007", "operator `{operator}`");
        assert!(!diagnostic.suggested_fix.is_empty());
    }
}

/// An operand pair that cannot combine is reported with the operator.
#[test]
fn rejects_operands_that_cannot_combine() {
    let diagnostic = parse(
        "from rivet import api\n\n@api.get(\"/n\", stories=[\"US-1\"])\ndef n(name: str) -> str:\n    return name - 1\n",
    )
    .expect_err("a string minus an int has no type");
    assert_eq!(diagnostic.error_code, "E1010");
}

/// A statement the subset does not carry is named in the diagnostic.
#[test]
fn rejects_an_unsupported_statement() {
    let diagnostic = parse(
        "from rivet import api\n\n@api.get(\"/n\", stories=[\"US-1\"])\ndef n() -> dict:\n    while True:\n        return {\"n\": 1}\n",
    )
    .expect_err("a while loop must fail");
    assert_eq!(diagnostic.error_code, "E1006");
    assert!(
        diagnostic.message.contains("while_statement"),
        "the diagnostic names the statement: {}",
        diagnostic.message
    );
}

/// A local is visible to the statements below it, and to nothing else.
#[test]
fn a_local_is_visible_below_its_binding() {
    let blueprint = parse(
        "from rivet import api\n\n@api.get(\"/n\", stories=[\"US-1\"])\ndef n(qty: int) -> int:\n    doubled = qty * 2\n    return doubled\n",
    )
    .expect("a local resolves below its binding");
    assert_eq!(blueprint.routes[0].body.len(), 2);
}

/// Returning a local of the wrong type is rejected.
#[test]
fn rejects_a_return_of_the_wrong_local_type() {
    let diagnostic = parse(
        "from rivet import api\n\n@api.get(\"/n\", stories=[\"US-1\"])\ndef n() -> int:\n    name = \"x\"\n    return name\n",
    )
    .expect_err("a str local does not satisfy an int response");
    assert_eq!(diagnostic.error_code, "E1010");
}

/// An assignment to anything but a plain name is rejected.
#[test]
fn rejects_an_assignment_to_a_field_or_subscript() {
    for target in ["x.y", "x[0]"] {
        let source = format!(
            "from rivet import api\n\n@api.get(\"/n\", stories=[\"US-1\"])\ndef n(qty: int) -> int:\n    {target} = 1\n    return qty\n"
        );
        let diagnostic = match parse(&source) {
            Ok(_) => panic!("assigning to `{target}` must fail"),
            Err(error) => error,
        };
        assert_eq!(diagnostic.error_code, "E1007", "target `{target}`");
    }
}
