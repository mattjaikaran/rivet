//! Tests for `rivet.toml` parsing and defaults.

use super::*;

#[test]
fn defaults_apply_without_a_file() {
    let config = RivetConfig::load(std::path::Path::new("/nonexistent"))
        .expect("missing file is not an error");
    assert_eq!(config.project.name, "app");
    assert_eq!(config.environments.development.host, "127.0.0.1");
    assert_eq!(config.environments.development.port, 3000);
}

#[test]
fn gauntlet_defaults_apply_without_a_section() {
    let config = RivetConfig::default();
    assert_eq!(config.gauntlet.max_complexity, 8);
    assert!(config.gauntlet.stories_required);
    assert!(config.gauntlet.strict_type_checking);
    assert_eq!(config.gauntlet.duplicate_code, Severity::Blocker);
    assert_eq!(config.gauntlet.dead_code, Severity::Warning);
}

#[test]
fn unknown_sections_are_ignored() {
    let raw = r#"
[project]
name = "orders"

[gauntlet]
max_complexity = 8

[rust_native_features]
compile_time_rbac = true
"#;
    let config: RivetConfig = toml::from_str(raw).expect("parse");
    assert_eq!(config.project.name, "orders");
    assert_eq!(config.environments.development.port, 3000);
}

#[test]
fn gauntlet_section_parses_thresholds_and_severities() {
    let raw = r#"
[gauntlet]
max_complexity = 5
stories_required = false
strict_type_checking = false
duplicate_code = "warn"
dead_code = "block"
"#;
    let config: RivetConfig = toml::from_str(raw).expect("parse");
    assert_eq!(config.gauntlet.max_complexity, 5);
    assert!(!config.gauntlet.stories_required);
    assert!(!config.gauntlet.strict_type_checking);
    assert_eq!(config.gauntlet.duplicate_code, Severity::Warning);
    assert_eq!(config.gauntlet.dead_code, Severity::Blocker);
}

#[test]
fn malformed_severity_word_fails_the_parse() {
    let raw = r#"
[gauntlet]
dead_code = "sometimes"
"#;
    let config: Result<RivetConfig, _> = toml::from_str(raw);
    assert!(config.is_err());
}

#[test]
fn plugin_section_parses_path_crate_and_version() {
    let raw = r#"
[plugins.auth-token]
path = "plugins/auth-token"

[plugins.audit-log]
crate = "rivet-plugin-custom"
version = "0.3"
"#;
    let config: RivetConfig = toml::from_str(raw).expect("parse");
    let auth = config.plugins.get("auth-token").expect("auth-token entry");
    assert_eq!(auth.path.as_deref(), Some("plugins/auth-token"));
    assert!(auth.version.is_none());
    assert_eq!(auth.crate_name("auth-token"), "rivet-plugin-auth-token");

    let audit = config.plugins.get("audit-log").expect("audit-log entry");
    assert_eq!(audit.version.as_deref(), Some("0.3"));
    assert!(audit.path.is_none());
    assert_eq!(audit.crate_name("audit-log"), "rivet-plugin-custom");
}

#[test]
fn plugin_section_is_empty_by_default() {
    let config: RivetConfig = toml::from_str("[project]\nname = \"orders\"\n").expect("parse");
    assert!(config.plugins.is_empty());
}

#[test]
fn transport_defaults_to_in_process() {
    let config = RivetConfig::default();
    assert_eq!(config.transport.mode, TransportMode::InProcess);
    assert_eq!(config.transport.grpc_port, 50051);
}

#[test]
fn transport_section_selects_grpc_and_its_port() {
    let raw = "[transport]\nmode = \"grpc\"\ngrpc_port = 7000\n";
    let config: RivetConfig = toml::from_str(raw).expect("parse");
    assert_eq!(config.transport.mode, TransportMode::Grpc);
    assert_eq!(config.transport.grpc_port, 7000);
}

#[test]
fn an_unknown_transport_mode_fails_the_parse() {
    let raw = "[transport]\nmode = \"sidecar\"\n";
    let config: Result<RivetConfig, _> = toml::from_str(raw);
    assert!(config.is_err());
}

#[test]
fn frontend_defaults_to_no_dist_and_client_side_routing() {
    let config = RivetConfig::default();
    assert!(config.frontend.dist.is_none());
    assert!(config.frontend.spa);
}

#[test]
fn frontend_section_parses_the_dist_directory_and_the_spa_flag() {
    let raw = "[frontend]\ndist = \"dist\"\nspa = false\n";
    let config: RivetConfig = toml::from_str(raw).expect("parse");
    assert_eq!(config.frontend.dist.as_deref(), Some("dist"));
    assert!(!config.frontend.spa);

    let raw = "[frontend]\ndist = \"frontend/build\"\n";
    let config: RivetConfig = toml::from_str(raw).expect("parse");
    assert_eq!(config.frontend.dist.as_deref(), Some("frontend/build"));
    assert!(config.frontend.spa, "spa defaults to true");
}

#[test]
fn the_admin_panel_is_off_by_default() {
    let config = RivetConfig::default();
    assert!(!config.admin.enabled);
}

#[test]
fn the_admin_section_enables_the_panel() {
    let config: RivetConfig = toml::from_str("[admin]\nenabled = true\n").expect("parse");
    assert!(config.admin.enabled);
}

#[test]
fn a_project_without_a_discovery_section_registers_nowhere() {
    let config = RivetConfig::default();
    assert!(config.discovery.backend.is_none());
    assert!(config.discovery.url().is_none());
    assert_eq!(config.discovery.service_name("orders"), "orders");
}

#[test]
fn discovery_section_selects_a_backend_and_its_defaults() {
    let raw = "[discovery]\nbackend = \"consul\"\n";
    let config: RivetConfig = toml::from_str(raw).expect("parse");
    assert_eq!(config.discovery.backend, Some(DiscoveryBackend::Consul));
    assert_eq!(
        config.discovery.url().as_deref(),
        Some("http://127.0.0.1:8500")
    );
    assert_eq!(
        config.discovery.service_name("orders"),
        "orders",
        "the project name is the default service name"
    );
    assert!(config.discovery.service_port.is_none());

    let raw = "[discovery]\nbackend = \"etcd\"\n";
    let config: RivetConfig = toml::from_str(raw).expect("parse");
    assert_eq!(config.discovery.backend, Some(DiscoveryBackend::Etcd));
    assert_eq!(
        config.discovery.url().as_deref(),
        Some("http://127.0.0.1:2379")
    );
}

#[test]
fn discovery_section_parses_url_name_and_port() {
    let raw = "[discovery]\nbackend = \"consul\"\nurl = \"http://consul.internal:8501\"\nservice_name = \"orders-api\"\nservice_port = 8080\n";
    let config: RivetConfig = toml::from_str(raw).expect("parse");
    assert_eq!(
        config.discovery.url().as_deref(),
        Some("http://consul.internal:8501")
    );
    assert_eq!(config.discovery.service_name("orders"), "orders-api");
    assert_eq!(config.discovery.service_port, Some(8080));
}

#[test]
fn an_unknown_discovery_backend_fails_the_parse() {
    let raw = "[discovery]\nbackend = \"nacos\"\n";
    let config: Result<RivetConfig, _> = toml::from_str(raw);
    assert!(config.is_err());
}
