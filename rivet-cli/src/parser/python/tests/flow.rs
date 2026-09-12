//! `for` and `match` in a handler body: the binding rules, the pattern rules,
//! and how each statement completes a handler that must return a value.

use super::*;
use rivet_core::ir::Stmt;

/// A loop binds its element name, and the binding takes the element type.
#[test]
fn parses_a_for_loop_over_a_list_literal() {
    let source = r#"
from rivet import api

@api.get("/n", stories=["US-1"])
def n() -> dict:
    total = 0
    for line in [1, 2, 3]:
        total = total + line
    return {"total": total}
"#;
    let blueprint = parse(source).expect("parse");
    let [
        Stmt::Assign { .. },
        Stmt::For {
            name,
            ty,
            iterable,
            body,
        },
        Stmt::Return(_),
    ] = blueprint.routes[0].body.as_slice()
    else {
        panic!("the body is a binding, a loop, and a return");
    };
    assert_eq!(name, "line");
    assert_eq!(ty, &TypeRef::Int, "the binding takes the element type");
    assert!(matches!(iterable, Expr::Array(items) if items.len() == 3));
    assert_eq!(body.len(), 1, "the loop owns its body");
    assert!(matches!(
        body[0],
        Stmt::Assign { ref name, .. } if name == "total"
    ));
}

/// A loop binding is not readable below the loop: Rust scopes the `let` to
/// the loop, so a name that escaped it would not compile.
#[test]
fn a_loop_binding_is_not_visible_below_the_loop() {
    let diagnostic = parse(
        "from rivet import api\n\n@api.get(\"/n\", stories=[\"US-1\"])\ndef n() -> dict:\n    for line in [1, 2, 3]:\n        pass\n    return {\"line\": line}\n",
    )
    .expect_err("the loop binding must not escape the loop");
    assert_eq!(diagnostic.error_code, "E1010");
}

/// A loop name that is already in scope is rejected, because Python would
/// overwrite it and keep the last element where Rust would keep the outer
/// value.
#[test]
fn rejects_a_loop_name_already_in_scope() {
    let diagnostic = parse(
        "from rivet import api\n\n@api.get(\"/n/{line}\", stories=[\"US-1\"])\ndef n(line: int) -> dict:\n    for line in [1, 2, 3]:\n        pass\n    return {\"line\": line}\n",
    )
    .expect_err("the loop name shadows a parameter");
    assert_eq!(diagnostic.error_code, "E1016");
    assert!(!diagnostic.suggested_fix.is_empty());
}

/// A loop body cannot reassign the loop's own binding: the generated `for`
/// binds immutably.
#[test]
fn rejects_an_assignment_to_the_loop_binding() {
    let diagnostic = parse(
        "from rivet import api\n\n@api.get(\"/n\", stories=[\"US-1\"])\ndef n() -> dict:\n    for line in [1, 2, 3]:\n        line = 5\n    return {}\n",
    )
    .expect_err("the loop binding is read-only");
    assert_eq!(diagnostic.error_code, "E1016");
}

/// A loop needs a list, and a non-list collection is reported with its type.
#[test]
fn rejects_a_loop_over_a_non_list() {
    let diagnostic = parse(
        "from rivet import api\n\n@api.get(\"/n\", stories=[\"US-1\"])\ndef n(qty: int) -> dict:\n    for line in qty:\n        pass\n    return {}\n",
    )
    .expect_err("an int is not iterable");
    assert_eq!(diagnostic.error_code, "E1010");
    assert_eq!(diagnostic.message.contains("int"), true);
}

/// Unpacking a loop value into several names is not carried yet, and the
/// diagnostic names the target.
#[test]
fn rejects_an_unpacking_loop_target() {
    let diagnostic = parse(
        "from rivet import api\n\n@api.get(\"/n\", stories=[\"US-1\"])\ndef n() -> dict:\n    for a, b in [1, 2]:\n        pass\n    return {}\n",
    )
    .expect_err("a tuple target must fail");
    assert_eq!(diagnostic.error_code, "E1007");
    assert!(
        diagnostic.message.contains("a, b"),
        "{}",
        diagnostic.message
    );
}

/// A `match` becomes arms in source order, with `None` for the wildcard.
#[test]
fn parses_a_match_with_a_final_wildcard() {
    let source = r#"
from rivet import api

@api.get("/grade/{score}", stories=["US-1"])
def grade(score: int) -> dict:
    match score:
        case 1:
            return {"grade": "low"}
        case 2:
            return {"grade": "mid"}
        case _:
            return {"grade": "high"}
"#;
    let blueprint = parse(source).expect("parse");
    let [Stmt::Match { subject, arms }] = blueprint.routes[0].body.as_slice() else {
        panic!("the body is one match");
    };
    assert_eq!(subject, &Expr::Ident("score".to_string()));
    assert_eq!(arms.len(), 3);
    assert_eq!(arms[0].0, Some(Expr::Int(1)));
    assert_eq!(arms[1].0, Some(Expr::Int(2)));
    assert_eq!(arms[2].0, None, "the wildcard is the final arm");
    assert_eq!(arms[0].1.len(), 1, "each arm owns its body");
}

/// A negative literal keeps its sign: the grammar puts the minus outside the
/// number node, so reading the number alone would answer `1` for `-1`.
#[test]
fn a_match_pattern_keeps_a_negative_sign() {
    let blueprint = parse(
        "from rivet import api\n\n@api.get(\"/n/{x}\", stories=[\"US-1\"])\ndef n(x: int) -> str:\n    match x:\n        case -1:\n            return \"neg\"\n        case _:\n            return \"other\"\n",
    )
    .expect("parse");
    let Stmt::Match { arms, .. } = &blueprint.routes[0].body[0] else {
        panic!("the body is one match");
    };
    assert_eq!(arms[0].0, Some(Expr::Int(-1)));
}

/// A `match` with no wildcard would emit a Rust `match` that does not cover
/// every value, so the parser refuses it rather than let cargo reject the
/// generated crate.
#[test]
fn rejects_a_match_without_a_wildcard() {
    let diagnostic = parse(
        "from rivet import api\n\n@api.get(\"/n/{x}\", stories=[\"US-1\"])\ndef n(x: int) -> dict:\n    match x:\n        case 1:\n            return {\"g\": \"a\"}\n    return {}\n",
    )
    .expect_err("a wildcard is required");
    assert_eq!(diagnostic.error_code, "E1017");
    assert!(!diagnostic.suggested_fix.is_empty());
}

/// The wildcard matches every value, so a later arm is unreachable.
#[test]
fn rejects_a_wildcard_that_is_not_last() {
    let diagnostic = parse(
        "from rivet import api\n\n@api.get(\"/n/{x}\", stories=[\"US-1\"])\ndef n(x: int) -> dict:\n    match x:\n        case _:\n            return {\"g\": \"a\"}\n        case 1:\n            return {\"g\": \"b\"}\n",
    )
    .expect_err("the wildcard must come last");
    assert_eq!(diagnostic.error_code, "E1017");
}

/// A pattern of the wrong type cannot compare with the subject.
#[test]
fn rejects_a_pattern_of_the_wrong_type() {
    let diagnostic = parse(
        "from rivet import api\n\n@api.get(\"/n/{x}\", stories=[\"US-1\"])\ndef n(x: int) -> dict:\n    match x:\n        case \"low\":\n            return {\"g\": \"a\"}\n        case _:\n            return {\"g\": \"b\"}\n",
    )
    .expect_err("a str pattern does not match an int subject");
    assert_eq!(diagnostic.error_code, "E1017");
}

/// Two equal patterns would make the second arm unreachable.
#[test]
fn rejects_a_repeated_pattern() {
    let diagnostic = parse(
        "from rivet import api\n\n@api.get(\"/n/{x}\", stories=[\"US-1\"])\ndef n(x: int) -> dict:\n    match x:\n        case 1:\n            return {\"g\": \"a\"}\n        case 1:\n            return {\"g\": \"b\"}\n        case _:\n            return {\"g\": \"c\"}\n",
    )
    .expect_err("a repeated pattern must fail");
    assert_eq!(diagnostic.error_code, "E1017");
}

/// A `dict` subject has no literal pattern the generated crate can compare.
#[test]
fn rejects_a_non_scalar_match_subject() {
    let diagnostic = parse(
        "from rivet import api\n\n@api.post(\"/n\", stories=[\"US-1\"])\ndef n(request: dict) -> dict:\n    match request:\n        case _:\n            return {}\n",
    )
    .expect_err("a dict subject must fail");
    assert_eq!(diagnostic.error_code, "E1010");
}

/// A `for` may run zero times, so it never completes a handler that must
/// return a value. A `match` completes through its wildcard arm.
#[test]
fn a_loop_never_completes_a_handler_that_must_return() {
    let diagnostic = parse(
        "from rivet import api\n\n@api.get(\"/n\", stories=[\"US-1\"])\ndef n() -> int:\n    total = 0\n    for line in [1, 2, 3]:\n        total = total + line\n",
    )
    .expect_err("a loop cannot satisfy the return check");
    assert_eq!(diagnostic.error_code, "E1010");

    parse(
        "from rivet import api\n\n@api.get(\"/n/{x}\", stories=[\"US-1\"])\ndef n(x: int) -> int:\n    match x:\n        case 1:\n            return 1\n        case _:\n            return 2\n",
    )
    .expect("a match with a wildcard completes");
}

/// A local keeps one type: the generated crate binds it once, so a rebinding
/// that changes the type would fail to compile there.
#[test]
fn rejects_a_rebinding_that_changes_the_type() {
    let diagnostic = parse(
        "from rivet import api\n\n@api.get(\"/n\", stories=[\"US-1\"])\ndef n() -> dict:\n    total = 1\n    total = \"x\"\n    return {\"total\": total}\n",
    )
    .expect_err("a local keeps one type");
    assert_eq!(diagnostic.error_code, "E1016");
    assert!(diagnostic.message.contains("int"), "{}", diagnostic.message);
}

/// A name bound inside a block is not readable below the block: Rust scopes
/// the `let` to the block, so a name that escaped would not compile.
#[test]
fn a_name_bound_in_a_branch_is_not_visible_below_it() {
    let diagnostic = parse(
        "from rivet import api\n\n@api.get(\"/n/{qty}\", stories=[\"US-1\"])\ndef n(qty: int) -> dict:\n    if qty > 1:\n        local = 1\n    else:\n        pass\n    return {\"local\": local}\n",
    )
    .expect_err("a branch local must not escape the branch");
    assert_eq!(diagnostic.error_code, "E1010");
}
