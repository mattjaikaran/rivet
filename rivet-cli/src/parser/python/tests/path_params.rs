//! Path parameters in the Python front end: a `{name}` placeholder binds to
//! the handler parameter of the same name, in path order.

use super::*;
use rivet_core::ir::{ResponseSpec, RouteParam, Stmt};

/// A `{name}` placeholder binds to the handler parameter of the same name and
/// becomes a typed path parameter, in path order.
#[test]
fn parses_path_parameters_in_path_order() {
    let source = "from rivet import api\n\n@api.get(\"/orders/{id}/items/{sku}\", stories=[\"US-1\"])\ndef get_item(id: int, sku: str) -> dict:\n    return {\"id\": id, \"sku\": sku}\n";
    let blueprint = parse(source).expect("parse");
    let route = &blueprint.routes[0];
    assert_eq!(
        route.path_params,
        vec![
            RouteParam {
                name: "id".to_string(),
                ty: TypeRef::Int,
            },
            RouteParam {
                name: "sku".to_string(),
                ty: TypeRef::String,
            },
        ]
    );
    assert_eq!(route.request, RequestSpec::None);
}

/// A path parameter and a JSON body coexist: the body is the one parameter
/// the route path does not name.
#[test]
fn parses_a_path_parameter_beside_a_json_body() {
    let source = r#"
from rivet import api

class Order:
    id: int
    note: str

@api.put("/orders/{id}", stories=["US-1"])
def update_order(id: int, request: Order) -> Order:
    return request
"#;
    let blueprint = parse(source).expect("parse");
    let route = &blueprint.routes[0];
    assert_eq!(
        route.path_params,
        vec![RouteParam {
            name: "id".to_string(),
            ty: TypeRef::Int,
        }]
    );
    assert_eq!(
        route.request,
        RequestSpec::Json {
            var: "request".to_string(),
            ty: TypeRef::Named("Order".to_string()),
        }
    );
}

/// A path parameter is typed like any other parameter, so a handler may
/// return it under a primitive annotation.
#[test]
fn a_path_parameter_can_be_the_response_value() {
    let blueprint = parse(
        "from rivet import api\n\n@api.get(\"/orders/{id}\")\ndef get_order(id: int) -> int:\n    return id\n",
    )
    .expect("parse");
    let route = &blueprint.routes[0];
    assert_eq!(route.response, ResponseSpec::Json(TypeRef::Int));
    assert_eq!(
        route.body,
        vec![Stmt::Return(Expr::Ident("id".to_string()))]
    );
}

/// A placeholder the handler does not declare is a parse error, not a route
/// that silently drops the segment.
#[test]
fn rejects_a_placeholder_without_a_handler_parameter() {
    let diagnostic = parse(
        "from rivet import api\n\n@api.get(\"/orders/{id}\")\ndef get_order() -> dict:\n    return {}\n",
    )
    .expect_err("must fail");
    assert_eq!(diagnostic.error_code, "E1015");
    assert!(
        diagnostic.message.contains("id"),
        "the diagnostic names the placeholder: {}",
        diagnostic.message
    );
    assert!(!diagnostic.suggested_fix.is_empty());
}

/// A repeated placeholder would bind two segments to one parameter.
#[test]
fn rejects_a_duplicate_placeholder() {
    let diagnostic = parse(
        "from rivet import api\n\n@api.get(\"/a/{id}/b/{id}\")\ndef get_thing(id: int) -> dict:\n    return {}\n",
    )
    .expect_err("must fail");
    assert_eq!(diagnostic.error_code, "E1015");
    assert!(!diagnostic.suggested_fix.is_empty());
}

/// A malformed placeholder, an unsafe name, and an unsupported type are all
/// rejected before the generator sees them.
#[test]
fn rejects_a_malformed_placeholder() {
    for path in [
        "/orders/{id",
        "/orders/id}",
        "/orders/{}",
        "/orders/{order-id}",
    ] {
        let source = format!(
            "from rivet import api\n\n@api.get(\"{path}\")\ndef get_order(id: int) -> dict:\n    return {{}}\n"
        );
        let diagnostic = match parse(&source) {
            Ok(_) => panic!("path `{path}` must be rejected"),
            Err(error) => error,
        };
        assert_eq!(diagnostic.error_code, "E1015", "path `{path}`");
        assert!(!diagnostic.suggested_fix.is_empty(), "path `{path}`");
    }
}

/// A path parameter must be a value the router can parse out of one segment.
#[test]
fn rejects_a_path_parameter_type_the_router_cannot_parse() {
    let diagnostic = parse(
        "from rivet import api\n\n@api.get(\"/orders/{id}\")\ndef get_order(id: dict) -> dict:\n    return {}\n",
    )
    .expect_err("must fail");
    assert_eq!(diagnostic.error_code, "E1015");
    assert!(
        diagnostic.message.contains("dict"),
        "{}",
        diagnostic.message
    );
    assert!(!diagnostic.suggested_fix.is_empty());
}

/// One parameter name may not be declared twice.
#[test]
fn rejects_a_duplicate_parameter_name() {
    let diagnostic = parse(
        "from rivet import api\n\n@api.post(\"/ping\")\ndef ping(request: dict, request: dict) -> dict:\n    return {}\n",
    )
    .expect_err("must fail");
    assert_eq!(diagnostic.error_code, "E1004");
    assert!(!diagnostic.suggested_fix.is_empty());
}

/// Path parameters do not lift the one-body limit.
#[test]
fn rejects_two_body_parameters_beside_a_path_parameter() {
    let diagnostic = parse(
        "from rivet import api\n\n@api.post(\"/orders/{id}\")\ndef update_order(id: int, a: dict, b: dict) -> dict:\n    return {}\n",
    )
    .expect_err("must fail");
    assert_eq!(diagnostic.error_code, "E1004");
    assert!(!diagnostic.suggested_fix.is_empty());
}
