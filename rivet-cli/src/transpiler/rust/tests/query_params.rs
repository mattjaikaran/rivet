//! Query parameters in the generated crate: the service signature, the axum
//! handler's `Query` extractor and its 400 bindings, the channel call, and the
//! gRPC payload.

use super::*;
use rivet_core::ir::{
    Expr, HttpMethod, RequestSpec, ResponseSpec, RouteDefinition, RouteParam, Stmt, TypeRef,
};

#[test]
fn query_parameters_render_the_service_channel_and_handler() {
    let project = generate_project(
        &query_params_blueprint(),
        &RivetConfig::default(),
        Path::new("."),
    )
    .expect("generate");

    // The service function carries the query parameters in declaration order.
    assert!(
        project
            .main_rs
            .contains("pub async fn search(page: i64, size: i64) -> serde_json::Value {"),
        "main_rs:\n{}",
        project.main_rs
    );

    // The handler extracts the query map after the state.
    assert!(
        project.main_rs.contains(
            "pub async fn search<C: channel::Channel>(super::State(channel): super::State<C>, super::Query(query): super::Query<std::collections::HashMap<String, String>>)"
        ),
        "main_rs:\n{}",
        project.main_rs
    );

    // A missing key and an unparseable value both answer 400.
    assert!(
        project
            .main_rs
            .contains("the query parameter `page` is missing"),
        "main_rs:\n{}",
        project.main_rs
    );
    assert!(
        project
            .main_rs
            .contains("the query parameter `page` must parse as int, got `{value}`"),
        "main_rs:\n{}",
        project.main_rs
    );

    // The in-process channel passes the query variables in declaration order.
    assert!(
        project
            .main_rs
            .contains("service::search(page, size).await"),
        "main_rs:\n{}",
        project.main_rs
    );
    assert!(
        project.main_rs.contains("channel.search(page, size)"),
        "main_rs:\n{}",
        project.main_rs
    );

    // The crate root imports `Query` because a route needs it.
    assert!(
        project.main_rs.contains("extract::{Json, Query, State}"),
        "main_rs:\n{}",
        project.main_rs
    );
}

#[test]
fn query_parameters_round_trip_over_grpc() {
    let project = generate_project(&query_params_blueprint(), &grpc_config(), Path::new("."))
        .expect("generate");

    // Two arguments serialize as a tuple...
    assert!(
        project
            .main_rs
            .contains("let payload = serde_json::to_string(&(page, size))"),
        "main_rs:\n{}",
        project.main_rs
    );
    // ...and the dispatch decodes the same tuple, then calls in order.
    assert!(
        project
            .main_rs
            .contains("let (page, size): (i64, i64) = serde_json::from_str(payload)"),
        "main_rs:\n{}",
        project.main_rs
    );
    assert!(
        project
            .main_rs
            .contains("service::search(page, size).await"),
        "main_rs:\n{}",
        project.main_rs
    );
}

/// A route with one string query parameter: `GET /find?term=x` +
/// `def find(term: str) -> dict`.
fn string_query_blueprint() -> ServiceBlueprint {
    ServiceBlueprint {
        name: "app".to_string(),
        structs: vec![],
        routes: vec![RouteDefinition {
            method: HttpMethod::Get,
            path: "/find".to_string(),
            path_params: vec![],
            query_params: vec![RouteParam {
                name: "term".to_string(),
                ty: TypeRef::String,
            }],
            handler_name: "find".to_string(),
            stories: vec!["US-014".to_string()],
            middlewares: vec![],
            request: RequestSpec::None,
            response: ResponseSpec::Json(TypeRef::Json),
            body: vec![Stmt::Return(Expr::Object(vec![(
                "term".to_string(),
                Expr::Ident("term".to_string()),
            )]))],
        }],
        dependencies: vec![],
    }
}

#[test]
fn a_string_query_parameter_takes_the_value_as_is() {
    let project = generate_project(
        &string_query_blueprint(),
        &RivetConfig::default(),
        Path::new("."),
    )
    .expect("generate");

    assert!(
        project
            .main_rs
            .contains("pub async fn find(term: String) -> serde_json::Value {"),
        "main_rs:\n{}",
        project.main_rs
    );
    // `str` is taken as-is, never parsed.
    assert!(
        project.main_rs.contains(
            "let term: String = match query.get(\"term\") {\n            Some(value) => value.clone(),"
        ),
        "main_rs:\n{}",
        project.main_rs
    );
}

/// A route with a path parameter, a query parameter, and a JSON body:
/// `PUT /orders/{id}?verbose=true` + `def update_order(id: int, verbose: bool, request: dict) -> dict`.
fn path_query_body_blueprint() -> ServiceBlueprint {
    ServiceBlueprint {
        name: "app".to_string(),
        structs: vec![],
        routes: vec![RouteDefinition {
            method: HttpMethod::Put,
            path: "/orders/{id}".to_string(),
            path_params: vec![RouteParam {
                name: "id".to_string(),
                ty: TypeRef::Int,
            }],
            query_params: vec![RouteParam {
                name: "verbose".to_string(),
                ty: TypeRef::Bool,
            }],
            handler_name: "update_order".to_string(),
            stories: vec!["US-015".to_string()],
            middlewares: vec![],
            request: RequestSpec::Json {
                var: "request".to_string(),
                ty: TypeRef::Json,
            },
            response: ResponseSpec::Json(TypeRef::Json),
            body: vec![Stmt::Return(Expr::Object(vec![
                ("id".to_string(), Expr::Ident("id".to_string())),
                ("verbose".to_string(), Expr::Ident("verbose".to_string())),
                ("request".to_string(), Expr::Ident("request".to_string())),
            ]))],
        }],
        dependencies: vec![],
    }
}

#[test]
fn path_query_and_body_parameters_order_correctly() {
    let project = generate_project(
        &path_query_body_blueprint(),
        &RivetConfig::default(),
        Path::new("."),
    )
    .expect("generate");

    // The service signature leads with the path, then the query, then the body.
    assert!(
        project.main_rs.contains(
            "pub async fn update_order(id: i64, verbose: bool, request: serde_json::Value) -> serde_json::Value {"
        ),
        "main_rs:\n{}",
        project.main_rs
    );
    // The extractor order: path, state, query, then body.
    assert!(
        project.main_rs.contains(
            "super::Path(id): super::Path<i64>, super::State(channel): super::State<C>, super::Query(query): super::Query<std::collections::HashMap<String, String>>, super::Json(request): super::Json<serde_json::Value>"
        ),
        "main_rs:\n{}",
        project.main_rs
    );
    // The channel call leads with the path, then the query, then the body.
    assert!(
        project
            .main_rs
            .contains("service::update_order(id, verbose, request).await"),
        "main_rs:\n{}",
        project.main_rs
    );
}
