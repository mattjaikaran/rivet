//! Tests for the generated discovery wiring.

use super::*;
use crate::config::Discovery;

fn config(section: &str) -> RivetConfig {
    let raw = format!("[project]\nname = \"orders\"\n{section}");
    toml::from_str(&raw).expect("parse")
}

#[test]
fn a_project_without_a_backend_keeps_the_plain_serve_statement() {
    let wiring = render(&RivetConfig::default()).expect("render");
    assert!(wiring.module.is_empty());
    assert!(wiring.register.is_empty());
    assert_eq!(wiring.serve, PLAIN_SERVE);
}

#[test]
fn consul_registers_the_name_and_port_with_the_agent() {
    let wiring = render(&config("[discovery]\nbackend = \"consul\"\n")).expect("render");
    // The registration body is one JSON literal in the generated crate, so
    // its quotes carry the Rust escape the source needs.
    assert!(
        wiring
            .module
            .contains(r#"\"ID\":\"orders\",\"Name\":\"orders\""#),
        "{wiring:?}"
    );
    assert!(wiring.module.contains(r#"\"Port\":3000"#), "{wiring:?}");
    assert!(wiring.module.contains("http://127.0.0.1:8500"));
    assert!(
        wiring
            .module
            .contains("/v1/agent/service/deregister/orders")
    );
    assert!(wiring.register.contains("discovery::register().await"));
    assert!(wiring.serve.contains("with_graceful_shutdown"));
    assert!(wiring.serve.contains("discovery::deregister().await"));
    assert!(
        !wiring.module.contains("serde_json::from_str"),
        "no etcd helper in consul mode"
    );
}

#[test]
fn discovery_reads_the_configured_url_name_and_port() {
    let wiring = render(&config(
        "[environments]\ndevelopment = { host = \"127.0.0.1\", port = 3000 }\n[discovery]\nbackend = \"consul\"\nurl = \"http://consul.internal:8501/agent\"\nservice_name = \"orders-api\"\nservice_port = 8080\n",
    ))
    .expect("render");
    assert!(wiring.module.contains("http://consul.internal:8501/agent"));
    assert!(
        wiring.module.contains(r#"\"ID\":\"orders-api\""#),
        "{wiring:?}"
    );
    assert!(wiring.module.contains(r#"\"Port\":8080"#), "{wiring:?}");
    assert!(wiring.module.contains("const SERVICE_PORT: u16 = 8080;"));
}

#[test]
fn etcd_registers_a_base64_key_on_a_lease() {
    let wiring = render(&config("[discovery]\nbackend = \"etcd\"\n")).expect("render");
    // `/rivet/services/orders` in standard base64.
    assert!(wiring.module.contains("L3JpdmV0L3NlcnZpY2VzL29yZGVycw=="));
    assert!(wiring.module.contains("/v3/lease/grant"));
    assert!(wiring.module.contains("/v3/kv/put"));
    assert!(wiring.module.contains("/v3/lease/revoke"));
    assert!(wiring.module.contains("LEASE_TTL"));
    assert!(wiring.module.contains("fn lease_id"));
    assert!(!wiring.module.contains("/v1/agent/service/register"));
}

#[test]
fn a_project_name_that_cannot_go_into_a_registry_path_is_rejected() {
    let error = render(&config(
        "[discovery]\nbackend = \"consul\"\nservice_name = \"orders/api\"\n",
    ))
    .expect_err("a slash cannot go into a registry path");
    assert_eq!(error.error_code, "E2005");
    assert!(error.message.contains("orders/api"));
    assert!(!error.suggested_fix.is_empty());
}

#[test]
fn the_service_name_defaults_to_the_project_and_the_port_to_development() {
    let config: RivetConfig = toml::from_str(
        "[project]\nname = \"orders\"\n[environments]\ndevelopment = { host = \"127.0.0.1\", port = 4444 }\n[discovery]\nbackend = \"consul\"\n",
    )
    .expect("parse");
    assert_eq!(Discovery::default().service_name("orders"), "orders");
    let wiring = render(&config).expect("render");
    assert!(wiring.module.contains("const SERVICE_PORT: u16 = 4444;"));
}
