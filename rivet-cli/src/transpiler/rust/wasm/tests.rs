//! Tests for the WebAssembly renderer.

use super::*;
use rivet_core::ir::{
    Expr, FieldDefinition, HttpMethod, RequestSpec, ResponseSpec, RouteDefinition,
    StructDefinition, TypeRef,
};

/// A blueprint with a parameterless `GET` and a body-taking `POST`.
fn blueprint() -> ServiceBlueprint {
    ServiceBlueprint {
        name: "app".to_string(),
        structs: vec![StructDefinition {
            name: "OrderCreate".to_string(),
            fields: vec![FieldDefinition {
                name: "sku".to_string(),
                type_ref: TypeRef::String,
                is_optional: false,
                is_borrowed: false,
            }],
        }],
        routes: vec![
            RouteDefinition {
                method: HttpMethod::Get,
                path: "/ping".to_string(),
                handler_name: "ping".to_string(),
                stories: vec!["US-001".to_string()],
                middlewares: vec![],
                request: RequestSpec::None,
                response: ResponseSpec::Json(TypeRef::Json),
                returns: vec![Expr::Object(vec![(
                    "status".to_string(),
                    Expr::Str("pong".to_string()),
                )])],
            },
            RouteDefinition {
                method: HttpMethod::Post,
                path: "/orders".to_string(),
                handler_name: "create_order".to_string(),
                stories: vec!["US-002".to_string()],
                middlewares: vec![],
                request: RequestSpec::Json {
                    var: "request".to_string(),
                    ty: TypeRef::Named("OrderCreate".to_string()),
                },
                response: ResponseSpec::Json(TypeRef::Named("OrderCreate".to_string())),
                returns: vec![Expr::Ident("request".to_string())],
            },
        ],
        dependencies: vec![],
    }
}

fn project() -> WasmProject {
    generate_wasm_project(&blueprint(), &RivetConfig::default()).expect("the crate renders")
}

#[test]
fn the_manifest_is_wasm_only() {
    let project = project();
    assert!(project.cargo_toml.contains("path = \"src/main.rs\""));
    assert!(project.cargo_toml.contains("serde_json = \"1\""));
    for native in ["axum", "tokio", "tonic", "rust-embed", "tower-http"] {
        assert!(
            !project.cargo_toml.contains(native),
            "a WASI module carries no {native}: {}",
            project.cargo_toml
        );
    }
}

#[test]
fn the_crate_reuses_the_service_layer() {
    let project = project();
    assert!(
        project.main_rs.contains("mod service {"),
        "the service layer is shared"
    );
    assert!(project.main_rs.contains("async fn ping()"));
    assert!(project.main_rs.contains("pub struct OrderCreate"));
    assert!(
        !project.main_rs.contains("axum::"),
        "the module names no server type"
    );
}

#[test]
fn the_dispatch_names_every_route() {
    let project = project();
    assert!(
        project.main_rs.contains("(\"GET\", \"/ping\")"),
        "{}",
        project.main_rs
    );
    assert!(project.main_rs.contains("(\"POST\", \"/orders\")"));
    assert!(
        project
            .main_rs
            .contains("const DECLARED_PATHS: &[&str] = &[\"/ping\", \"/orders\"]")
    );
    assert!(project.main_rs.contains("\"/ping\" => \"GET\""));
    assert!(
        project.main_rs.contains("\"/orders\" => \"POST\""),
        "{}",
        project.main_rs
    );
}

#[test]
fn a_body_route_deserializes_its_declared_type() {
    let project = project();
    assert!(
        project
            .main_rs
            .contains("let request: OrderCreate = match body {"),
        "{}",
        project.main_rs
    );
    assert!(
        project
            .main_rs
            .contains("service::create_order(request).await")
    );
}

#[test]
fn the_module_reads_a_request_and_writes_an_envelope() {
    let project = project();
    assert!(project.main_rs.contains("fn main()"));
    assert!(project.main_rs.contains("read_to_string(&mut raw)"));
    assert!(project.main_rs.contains("write_all(envelope.as_bytes())"));
    assert!(
        project.main_rs.contains("\"status\""),
        "the envelope carries the status"
    );
    assert!(
        !project.main_rs.contains("@@"),
        "every template token is substituted"
    );
}
