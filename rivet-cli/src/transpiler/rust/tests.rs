use super::*;
use crate::config::PluginConfig;
use crate::test_support::ScratchDir;
use rivet_core::ir::HttpMethod;
use std::fs;

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
    let project = generate_project(&ping_blueprint(), &config, Path::new(".")).expect("generate");
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
    let project = generate_project(&blueprint, &config, Path::new(".")).expect("generate");
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

#[test]
fn configured_plugins_generate_one_dependency_and_one_install_call() {
    let dir = ScratchDir::new("generate-plugin");
    fs::create_dir_all(dir.join("plugins/auth-token")).expect("create plugin dir");
    fs::write(dir.join("plugins/auth-token/Cargo.toml"), "[package]\n").expect("write manifest");
    let mut config = RivetConfig::default();
    config.plugins.insert(
        "auth-token".to_string(),
        PluginConfig {
            crate_name: None,
            path: Some("plugins/auth-token".to_string()),
            version: None,
        },
    );

    let project = generate_project(&ping_blueprint(), &config, &dir).expect("generate");

    assert!(
        project
            .cargo_toml
            .contains("rivet-plugin-auth-token = { path = \"../plugins/auth-token\" }"),
        "cargo_toml:\n{}",
        project.cargo_toml
    );
    assert!(
        project.main_rs.contains(
            "    // Plugin: auth-token\n    let app = rivet_plugin_auth_token::install(app);"
        ),
        "main_rs:\n{}",
        project.main_rs
    );
    assert_eq!(
        project.main_rs.matches("::install(app)").count(),
        1,
        "one monomorphized call per plugin"
    );
    assert!(
        !project.main_rs.contains("dyn "),
        "the generated app must not use trait objects"
    );
}

#[test]
fn a_project_without_plugins_generates_no_plugin_wiring() {
    let project = generate_project(&ping_blueprint(), &RivetConfig::default(), Path::new("."))
        .expect("generate");
    assert!(
        !project.main_rs.contains("install(app)"),
        "{}",
        project.main_rs
    );
    assert!(!project.cargo_toml.contains("Plugins, composed"));
    assert!(project.main_rs.contains("    let app = Router::new()"));
}
