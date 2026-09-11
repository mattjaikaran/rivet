//! Tests for query-parameter dispatch in the WebAssembly renderer.

use super::*;
use rivet_core::ir::{
    Expr, HttpMethod, RequestSpec, ResponseSpec, RouteDefinition, RouteParam, TypeRef,
};

/// A blueprint with three query-reading routes: two integer parameters, one
/// string parameter, and one path-plus-query route for argument ordering.
fn blueprint_with_query() -> ServiceBlueprint {
    ServiceBlueprint {
        name: "app".to_string(),
        structs: vec![],
        routes: vec![
            RouteDefinition {
                method: HttpMethod::Get,
                path: "/search".to_string(),
                path_params: vec![],
                query_params: vec![
                    RouteParam {
                        name: "page".to_string(),
                        ty: TypeRef::Int,
                    },
                    RouteParam {
                        name: "size".to_string(),
                        ty: TypeRef::Int,
                    },
                ],
                handler_name: "search".to_string(),
                stories: vec!["US-100".to_string()],
                middlewares: vec![],
                request: RequestSpec::None,
                response: ResponseSpec::Json(TypeRef::Json),
                returns: vec![Expr::Object(vec![
                    ("page".to_string(), Expr::Ident("page".to_string())),
                    ("size".to_string(), Expr::Ident("size".to_string())),
                ])],
            },
            RouteDefinition {
                method: HttpMethod::Get,
                path: "/greet".to_string(),
                path_params: vec![],
                query_params: vec![RouteParam {
                    name: "name".to_string(),
                    ty: TypeRef::String,
                }],
                handler_name: "greet".to_string(),
                stories: vec!["US-101".to_string()],
                middlewares: vec![],
                request: RequestSpec::None,
                response: ResponseSpec::Json(TypeRef::Json),
                returns: vec![Expr::Object(vec![(
                    "name".to_string(),
                    Expr::Ident("name".to_string()),
                )])],
            },
            RouteDefinition {
                method: HttpMethod::Get,
                path: "/orders/{id}".to_string(),
                path_params: vec![RouteParam {
                    name: "id".to_string(),
                    ty: TypeRef::Int,
                }],
                query_params: vec![RouteParam {
                    name: "verbose".to_string(),
                    ty: TypeRef::Bool,
                }],
                handler_name: "get_order".to_string(),
                stories: vec!["US-102".to_string()],
                middlewares: vec![],
                request: RequestSpec::None,
                response: ResponseSpec::Json(TypeRef::Json),
                returns: vec![Expr::Object(vec![
                    ("id".to_string(), Expr::Ident("id".to_string())),
                    ("verbose".to_string(), Expr::Ident("verbose".to_string())),
                ])],
            },
        ],
        dependencies: vec![],
    }
}

fn project_with_query() -> WasmProject {
    generate_wasm_project(&blueprint_with_query(), &RivetConfig::default())
        .expect("the crate renders")
}

/// The dispatch splits the query off the path before matching, so the path
/// pattern and the segment comparison see the path only.
#[test]
fn the_dispatch_splits_the_query_and_matches_the_path() {
    let project = project_with_query();
    assert!(
        project.main_rs.contains("path.split_once('?')"),
        "the dispatch splits the query off the path:\n{}",
        project.main_rs
    );
    assert!(
        project
            .main_rs
            .contains("rivet_path_matches(&segments, \"/search\")"),
        "matching sees the path only, not the query:\n{}",
        project.main_rs
    );
}

/// A query route binds and parses each value, answers 400 for a missing
/// parameter and for an unparseable one, and passes the query arguments to
/// the service in declaration order.
#[test]
fn a_query_route_binds_and_parses_each_value() {
    let project = project_with_query();
    assert!(
        project
            .main_rs
            .contains("let page: i64 = match rivet_query_value(query, \"page\") {"),
        "a numeric parameter parses from the query in place:\n{}",
        project.main_rs
    );
    assert!(
        project
            .main_rs
            .contains("service::search(page, size).await"),
        "the query arguments lead the call in declaration order:\n{}",
        project.main_rs
    );
    assert!(
        project
            .main_rs
            .contains("the query parameter `page` is missing"),
        "a missing parameter answers 400 naming it:\n{}",
        project.main_rs
    );
    assert!(
        project
            .main_rs
            .contains("the query parameter `page` must parse as int"),
        "an unparseable value answers 400 naming the declared type:\n{}",
        project.main_rs
    );
}

/// A string query parameter takes the decoded value as-is, and the query
/// decoder maps `+` to a space, unlike the path decoder.
#[test]
fn a_string_query_parameter_takes_the_value_and_plus_is_a_space() {
    let project = project_with_query();
    assert!(
        project
            .main_rs
            .contains("let name: String = match rivet_query_value(query, \"name\") {"),
        "a string parameter takes the decoded value as-is:\n{}",
        project.main_rs
    );
    assert!(
        project.main_rs.contains("bytes[index] == b'+'"),
        "the query decoder maps `+` to a space:\n{}",
        project.main_rs
    );
    assert!(
        project.main_rs.contains("decoded.push(b' ')"),
        "the query decoder maps `+` to a space:\n{}",
        project.main_rs
    );
}

/// A route with both path and query parameters passes path arguments first,
/// then query arguments.
#[test]
fn a_path_and_query_route_orders_path_then_query_args() {
    let project = project_with_query();
    assert!(
        project
            .main_rs
            .contains("service::get_order(id, verbose).await"),
        "path arguments lead, then query arguments:\n{}",
        project.main_rs
    );
}
