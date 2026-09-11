//! Query parameters in the Python front end: a parameter the route path does
//! not name reads from the query string when its type is a primitive, and is
//! the JSON request body otherwise.

use super::*;
use rivet_core::ir::RouteParam;

/// A primitive the path does not name is a query parameter, in declaration
/// order, and the route keeps no body.
#[test]
fn parses_primitive_parameters_as_query() {
    let source = "from rivet import api\n\n@api.get(\"/search\", stories=[\"US-1\"])\ndef search(page: int, size: int, name: str, exact: bool) -> dict:\n    return {\"page\": page, \"size\": size, \"name\": name, \"exact\": exact}\n";
    let blueprint = parse(source).expect("parse");
    let route = &blueprint.routes[0];
    assert!(route.path_params.is_empty());
    assert_eq!(
        route.query_params,
        vec![
            RouteParam {
                name: "page".to_string(),
                ty: TypeRef::Int,
            },
            RouteParam {
                name: "size".to_string(),
                ty: TypeRef::Int,
            },
            RouteParam {
                name: "name".to_string(),
                ty: TypeRef::String,
            },
            RouteParam {
                name: "exact".to_string(),
                ty: TypeRef::Bool,
            },
        ]
    );
    assert_eq!(route.request, RequestSpec::None);
}

/// A `dict` parameter the path does not name stays the JSON body.
#[test]
fn a_dict_parameter_is_still_the_body() {
    let blueprint = parse(
        "from rivet import api\n\n@api.post(\"/echo\")\ndef echo(request: dict) -> dict:\n    return {\"echo\": request}\n",
    )
    .expect("parse");
    let route = &blueprint.routes[0];
    assert!(route.query_params.is_empty());
    assert_eq!(
        route.request,
        RequestSpec::Json {
            var: "request".to_string(),
            ty: TypeRef::Json,
        }
    );
}

/// A `float` reads from the query string too.
#[test]
fn parses_a_float_query_parameter() {
    let blueprint = parse(
        "from rivet import api\n\n@api.get(\"/price\")\ndef price(rate: float) -> dict:\n    return {\"rate\": rate}\n",
    )
    .expect("parse");
    assert_eq!(
        blueprint.routes[0].query_params,
        vec![RouteParam {
            name: "rate".to_string(),
            ty: TypeRef::Float,
        }]
    );
}

/// A path parameter, a query parameter, and a body divide by position and
/// type: the path names one, a primitive is the query, and the DTO is the
/// body.
#[test]
fn parses_a_path_parameter_a_query_parameter_and_a_body_together() {
    let source = r#"
from rivet import api

class OrderUpdate:
    note: str

@api.put("/orders/{id}", stories=["US-1"])
def update_order(id: int, verbose: bool, request: OrderUpdate) -> dict:
    return {"id": id, "verbose": verbose}
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
        route.query_params,
        vec![RouteParam {
            name: "verbose".to_string(),
            ty: TypeRef::Bool,
        }]
    );
    assert_eq!(
        route.request,
        RequestSpec::Json {
            var: "request".to_string(),
            ty: TypeRef::Named("OrderUpdate".to_string()),
        }
    );
}

/// A parameter may not reuse a name the generated handler binds as a
/// parameter: Rust rejects two parameters bound to one name (`E0415`), so the
/// generated crate would not compile.
///
/// Only the two names the generator binds in the parameter list are reserved.
/// A name the handler merely reads into a local is legal, because shadowing a
/// local is allowed — `query` is the one that matters here.
#[test]
fn rejects_a_parameter_that_collides_with_a_handler_binding() {
    for name in ["channel", "bytes"] {
        let source = format!(
            "from rivet import api\n\n@api.get(\"/ping\")\ndef ping({name}: str) -> dict:\n    return {{}}\n"
        );
        let diagnostic = match parse(&source) {
            Ok(_) => panic!("parameter `{name}` collides with a generated binding"),
            Err(error) => error,
        };
        assert_eq!(diagnostic.error_code, "E1013", "parameter `{name}`");
        assert!(
            diagnostic.message.contains(name),
            "the diagnostic names the parameter: {}",
            diagnostic.message
        );
        assert!(!diagnostic.suggested_fix.is_empty());
    }
}

/// A parameter named `query` is legal: only a name bound in the parameter
/// list is reserved.
#[test]
fn a_parameter_named_query_is_accepted() {
    let blueprint = parse(
        "from rivet import api\n\n@api.get(\"/ping\")\ndef ping(query: str, page: int) -> dict:\n    return {\"q\": query, \"page\": page}\n",
    )
    .expect("`query` is not a generated binding");
    assert_eq!(blueprint.routes[0].query_params.len(), 2);
}

/// A list parameter the path does not name is a body, so it does not become a
/// query parameter, and two of them are rejected.
#[test]
fn rejects_two_list_parameters() {
    let diagnostic = parse(
        "from rivet import api\n\n@api.post(\"/e\")\ndef e(a: List[str], b: List[str]) -> dict:\n    return {}\n",
    )
    .expect_err("must fail");
    assert_eq!(diagnostic.error_code, "E1004");
}
