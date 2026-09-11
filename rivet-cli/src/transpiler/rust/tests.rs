use super::*;
use crate::config::{PluginConfig, Transport, TransportMode};
use crate::test_support::ScratchDir;
use rivet_core::ir::{HttpMethod, ResponseSpec};
use std::fs;

mod features;
mod fixtures;

use fixtures::*;

#[test]
fn crate_name_is_sanitized() {
    assert_eq!(crate_name("my-api"), "my-api");
    assert_eq!(crate_name("My Cool API!"), "my-cool-api");
    assert_eq!(crate_name(""), "app");
    assert_eq!(crate_name("9lives"), "app-9lives");
}

#[test]
fn renders_the_service_layer_the_channel_and_the_router() {
    let project = generate_project(&ping_blueprint(), &RivetConfig::default(), Path::new("."))
        .expect("generate");

    // The route logic lives in the transport-free service layer.
    assert!(
        project.main_rs.contains("mod service {"),
        "main_rs:\n{}",
        project.main_rs
    );
    assert!(
        project
            .main_rs
            .contains("pub async fn ping() -> serde_json::Value {"),
        "main_rs:\n{}",
        project.main_rs
    );
    assert!(
        project
            .main_rs
            .contains("json_obj(vec![(\"status\", serde_json::Value::from(\"pong\"))])"),
        "main_rs:\n{}",
        project.main_rs
    );

    // The channel is one typed method per route, and the handler calls it.
    assert!(project.main_rs.contains("pub trait Channel {"));
    assert!(
        project
            .main_rs
            .contains("fn ping(&self) -> impl std::future::Future<Output = Result<serde_json::Value, String>> + Send;")
    );
    assert!(
        project
            .main_rs
            .contains("async fn ping<C: channel::Channel>(State(channel): State<C>)")
    );

    // The router registers the concrete transport the config selected.
    assert!(
        project
            .main_rs
            .contains(".route(\"/ping\", get(ping::<channel::InProcess>))"),
        "main_rs:\n{}",
        project.main_rs
    );
    assert!(project.main_rs.contains(".with_state(channel::InProcess);"));
    assert!(project.cargo_toml.contains("name = \"app\""));
}

#[test]
fn in_process_mode_generates_no_grpc_code() {
    let project = generate_project(&ping_blueprint(), &RivetConfig::default(), Path::new("."))
        .expect("generate");
    assert!(
        !project.main_rs.contains("tonic"),
        "the monolith must carry no transport code"
    );
    assert!(!project.main_rs.contains("Grpc"));
    assert!(!project.cargo_toml.contains("tonic"));
    assert!(!project.cargo_toml.contains("tokio-stream"));
}

#[test]
fn grpc_mode_generates_the_channel_client_and_server() {
    let project =
        generate_project(&ping_blueprint(), &grpc_config(), Path::new(".")).expect("generate");

    assert!(project.main_rs.contains("pub struct Grpc {"));
    assert!(
        project
            .main_rs
            .contains("pub async fn connect(endpoint: String)")
    );
    assert!(
        project
            .main_rs
            .contains("tonic::codec::Codec for JsonCodec")
    );
    assert!(
        project
            .main_rs
            .contains("const NAME: &'static str = \"rivet.Channel\";")
    );
    assert!(
        project
            .main_rs
            .contains("pub async fn serve(listener: tokio::net::TcpListener)")
    );
    assert!(project.main_rs.contains("channel::Grpc::connect"));
    assert!(project.main_rs.contains(".with_state(channel);"));
    assert!(!project.main_rs.contains("dyn channel::Channel"));

    // The generated manifest gains the transport's crates and no build step.
    assert!(project.cargo_toml.contains("tonic = { version = \"0.12\""));
    assert!(
        project
            .cargo_toml
            .contains("tokio-stream = { version = \"0.1\", features = [\"net\"] }")
    );
    assert!(!project.cargo_toml.contains("prost ="));
    assert!(!project.cargo_toml.contains("tonic-build"));
}

#[test]
fn renders_a_dto_response_through_the_service_layer() {
    let project = generate_project(&dto_blueprint(), &RivetConfig::default(), Path::new("."))
        .expect("generate");

    assert!(project.main_rs.contains("pub struct OrderResponse"));
    assert!(
        project
            .main_rs
            .contains("pub async fn create_order() -> OrderResponse {"),
        "dto main_rs:\n{}",
        project.main_rs
    );
    assert!(project.main_rs.contains("status: \"ok\""));
    assert!(
        project
            .main_rs
            .contains("fn create_order(&self) -> impl std::future::Future<Output = Result<OrderResponse, String>> + Send;")
    );
    assert!(
        project
            .main_rs
            .contains("Result<Json<OrderResponse>, (axum::http::StatusCode, String)>"),
        "dto main_rs:\n{}",
        project.main_rs
    );
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

#[test]
fn a_configured_frontend_build_is_embedded_and_served_as_the_fallback() {
    let dir = ScratchDir::new("generate-assets");
    fs::create_dir_all(dir.join("dist")).expect("create dist");
    let mut config = RivetConfig::default();
    config.frontend.dist = Some("dist".to_string());

    let project = generate_project(&ping_blueprint(), &config, &dir).expect("generate");

    assert!(
        project.main_rs.contains("#[folder = \"../dist\"]"),
        "main_rs:\n{}",
        project.main_rs
    );
    assert!(
        project.main_rs.contains("mod assets {"),
        "{}",
        project.main_rs
    );
    assert!(project.main_rs.contains(".fallback(assets::serve)"));
    assert!(
        project
            .main_rs
            .contains("if wants_index(&method, &uri, &headers)"),
        "the default config serves one page for every navigation"
    );
    assert!(
        project.cargo_toml.contains("rust-embed = "),
        "cargo_toml:\n{}",
        project.cargo_toml
    );
    assert!(project.cargo_toml.contains("percent-encoding = \"2\""));
    assert!(
        project
            .main_rs
            .contains("app.layer(tower_http::compression::CompressionLayer::new())"),
        "every response is compressed: {}",
        project.main_rs
    );
    assert!(project.cargo_toml.contains("tower-http = "));
    assert_eq!(
        project.assets,
        AssetEmbedding::Embedded {
            dir: dir.join("dist"),
            folder: "../dist".to_string(),
        }
    );
}

#[test]
fn a_project_without_a_frontend_build_has_no_asset_wiring() {
    let project = generate_project(&ping_blueprint(), &RivetConfig::default(), Path::new("."))
        .expect("generate");

    assert_eq!(project.assets, AssetEmbedding::None);
    assert!(
        !project.main_rs.contains("mod assets"),
        "{}",
        project.main_rs
    );
    assert!(!project.main_rs.contains("fallback"));
    assert!(!project.cargo_toml.contains("rust-embed"));
}

#[test]
fn a_configured_but_missing_frontend_build_is_not_embedded() {
    let dir = ScratchDir::new("generate-assets-missing");
    let mut config = RivetConfig::default();
    config.frontend.dist = Some("dist".to_string());

    let project = generate_project(&ping_blueprint(), &config, &dir).expect("generate");

    assert_eq!(project.assets, AssetEmbedding::Missing(dir.join("dist")));
    assert!(
        !project.main_rs.contains("mod assets"),
        "{}",
        project.main_rs
    );
    assert!(
        !project.cargo_toml.contains("rust-embed"),
        "{}",
        project.cargo_toml
    );
}
