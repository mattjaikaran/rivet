use super::*;
use rivet_core::ir::HttpMethod;

fn ping_blueprint() -> ServiceBlueprint {
    ServiceBlueprint {
        name: "app".to_string(),
        structs: vec![],
        routes: vec![RouteDefinition {
            method: HttpMethod::Get,
            path: "/ping".to_string(),
            handler_name: "ping".to_string(),
            stories: vec![],
            middlewares: vec![],
            request: RequestSpec::None,
            response: ResponseSpec::Json(TypeRef::Json),
            returns: vec![Expr::Object(vec![(
                "status".to_string(),
                Expr::Str("pong".to_string()),
            )])],
        }],
        dependencies: vec![],
    }
}

#[test]
fn crate_name_is_sanitized() {
    assert_eq!(crate_name("my-api"), "my-api");
    assert_eq!(crate_name("My Cool API!"), "my-cool-api");
    assert_eq!(crate_name(""), "app");
    assert_eq!(crate_name("9lives"), "app-9lives");
}

#[test]
fn renders_ping_route() {
    let config = RivetConfig::default();
    let project = generate_project(&ping_blueprint(), &config).expect("generate");
    assert!(project.main_rs.contains(".route(\"/ping\", get(ping))"));
    assert!(
        project
            .main_rs
            .contains("async fn ping() -> Json<serde_json::Value>"),
        "ping main_rs:\n{}",
        project.main_rs
    );
    assert!(
        project
            .main_rs
            .contains("json_obj(vec![(\"status\", serde_json::Value::from(\"pong\"))])")
    );
    assert!(project.cargo_toml.contains("name = \"app\""));
}

#[test]
fn renders_dto_response() {
    let blueprint = ServiceBlueprint {
        name: "app".to_string(),
        structs: vec![StructDefinition {
            name: "OrderResponse".to_string(),
            fields: vec![FieldDefinition {
                name: "status".to_string(),
                type_ref: TypeRef::String,
                is_optional: false,
                is_borrowed: false,
            }],
        }],
        routes: vec![RouteDefinition {
            method: HttpMethod::Post,
            path: "/orders".to_string(),
            handler_name: "create_order".to_string(),
            stories: vec!["US-123".to_string()],
            middlewares: vec![],
            request: RequestSpec::None,
            response: ResponseSpec::Json(TypeRef::Named("OrderResponse".to_string())),
            returns: vec![Expr::Construct {
                ty: "OrderResponse".to_string(),
                args: vec![("status".to_string(), Expr::Str("ok".to_string()))],
            }],
        }],
        dependencies: vec![],
    };
    let config = RivetConfig::default();
    let project = generate_project(&blueprint, &config).expect("generate");
    assert!(project.main_rs.contains("pub struct OrderResponse"));
    assert!(
        project
            .main_rs
            .contains("async fn create_order() -> Json<OrderResponse>"),
        "dto main_rs:\n{}",
        project.main_rs
    );
    assert!(project.main_rs.contains("status: \"ok\""));
}
