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

/// The serde bridge for a fixed-size array field, emitted only when a DTO
/// declares one.
///
/// serde derives `Serialize` and `Deserialize` for arrays up to 32 elements,
/// so a larger `[T; N]` needs its own impls. Serializing goes through a
/// slice, and deserializing builds the array from a `Vec`, so the field's
/// Rust type stays `[T; N]` at every size.
pub(super) const FIXED_ARRAY: &str = r#"/// Serde support for a fixed-size array field.
///
/// A DTO uses it as `#[serde(with = "fixed_array")]` on the array field.
mod fixed_array {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    /// Write the array as a JSON sequence, through a slice.
    pub(super) fn serialize<S, T, const N: usize>(
        value: &[T; N],
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
        T: Serialize,
    {
        value.as_slice().serialize(serializer)
    }

    /// Read a JSON sequence into the array, and reject a length that does
    /// not match the declaration.
    pub(super) fn deserialize<'de, D, T, const N: usize>(
        deserializer: D,
    ) -> Result<[T; N], D::Error>
    where
        D: Deserializer<'de>,
        T: Deserialize<'de>,
    {
        let values = Vec::<T>::deserialize(deserializer)?;
        values.try_into().map_err(|values: Vec<T>| {
            serde::de::Error::invalid_length(values.len(), &"an array of the declared length")
        })
    }
}

"#;

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
