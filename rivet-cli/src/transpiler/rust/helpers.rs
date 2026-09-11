//! The runtime helpers every generated crate carries.
//!
//! The service layer renders `json_obj` and `json_number` calls for handlers
//! that return dicts and floats, so both targets must emit the same two
//! functions. They live here once, and each target adds its own transport
//! helpers beside them.

/// The helpers shared by the native and WebAssembly targets.
///
/// Both functions carry `#[allow(dead_code)]`: a blueprint may not use one,
/// and an unused private function is a warning in the generated crate.
pub(super) const SHARED: &str = "\
#[allow(dead_code)]
fn json_obj(pairs: Vec<(&'static str, serde_json::Value)>) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    for (key, value) in pairs {
        map.insert(key.to_string(), value);
    }
    serde_json::Value::Object(map)
}

#[allow(dead_code)]
fn json_number(value: f64) -> serde_json::Value {
    serde_json::Number::from_f64(value)
        .map(serde_json::Value::Number)
        .expect(\"finite float literal\")
}

";

/// The helper the native target adds: a channel failure maps to `502`.
pub(super) const CHANNEL_ERROR: &str = "\
/// Map a channel failure to `502` with the reason.
#[allow(dead_code)]
fn channel_error(detail: String) -> (axum::http::StatusCode, String) {
    tracing::error!(error = %detail, \"service channel call failed\");
    (axum::http::StatusCode::BAD_GATEWAY, detail)
}

";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shared_helpers_name_no_transport() {
        for helper in [SHARED, CHANNEL_ERROR] {
            assert!(!helper.contains("tokio::"), "{helper}");
        }
        assert!(SHARED.contains("fn json_obj"));
        assert!(SHARED.contains("fn json_number"));
        assert!(
            !SHARED.contains("axum"),
            "the shared helpers stay transport-free"
        );
        assert!(CHANNEL_ERROR.contains("axum::http::StatusCode"));
    }
}
