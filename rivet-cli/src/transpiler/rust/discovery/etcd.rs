//! The etcd half of service discovery: a lease, its key, and its keeper.
//!
//! etcd registers a service by putting a key on a lease. The lease expires
//! if nothing renews it, so a crashed app leaves the registry on its own.
//! etcd has no single-request lease refresh, so a keeper task grants a fresh
//! lease, moves the key onto it, and releases the old lease. The key is
//! therefore never absent while the app runs.

/// The etcd registration, deregistration, and helpers for one service.
pub(super) fn parts(name: &str, port: u16) -> (String, String, String) {
    let key = base64(format!("/rivet/services/{name}").as_bytes());
    let value = base64(format!("{{\"name\":\"{name}\",\"port\":{port}}}").as_bytes());

    // A plain const, not a `format!` template: the generated Rust keeps its
    // own braces, and the two tokens below are the only substitutions.
    let register = REGISTER
        .replace("@@KEY@@", &super::super::rust_str(&key))
        .replace("@@VALUE@@", &super::super::rust_str(&value));
    let deregister = "    /// Remove this service from etcd by releasing its lease.\n    pub(super) async fn deregister() -> Result<(), String> {\n        STOPPED.store(true, std::sync::atomic::Ordering::Relaxed);\n        let id = LEASE.swap(0, std::sync::atomic::Ordering::Relaxed);\n        if id == 0 {\n            return Ok(());\n        }\n        revoke(id).await\n    }\n".to_string();
    (register, deregister, HELPERS.to_string())
}

/// The etcd half of the module: the lease, the key, and the keeper that
/// renews it. `@@KEY@@` and `@@VALUE@@` carry the base64 constants.
const REGISTER: &str = r#"    /// The lease this app holds, and whether the app has stopped.
    static LEASE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    static STOPPED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

    /// How long a lease lives without a renewal.
    const LEASE_TTL: u64 = 60;

    /// This service's key and value. The v3 JSON gateway encodes byte
    /// strings as base64.
    const KEY: &str = @@KEY@@;
    const VALUE: &str = @@VALUE@@;

    /// The lease request.
    const GRANT_BODY: &str = "{\"TTL\":60}";

    /// Register this service with etcd: grant a lease, attach the key to
    /// it, then keep the lease alive.
    pub(super) async fn register() -> Result<String, String> {
        grant_and_put().await?;
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(LEASE_TTL / 3)).await;
                if STOPPED.load(std::sync::atomic::Ordering::Relaxed) {
                    return;
                }
                let previous = LEASE.load(std::sync::atomic::Ordering::Relaxed);
                if grant_and_put().await.is_err() {
                    continue;
                }
                let _ = revoke(previous).await;
            }
        });
        Ok(format!(
            "registered {SERVICE_NAME} on port {SERVICE_PORT} with etcd"
        ))
    }

    /// Grant a lease and attach this service's key to it.
    async fn grant_and_put() -> Result<u64, String> {
        let granted = request("POST", "/v3/lease/grant", Some(GRANT_BODY)).await?;
        let id = lease_id(&granted)?;
        let body = format!("{{\"key\":{KEY},\"value\":{VALUE},\"lease\":{id}}}");
        request("POST", "/v3/kv/put", Some(&body)).await?;
        LEASE.store(id, std::sync::atomic::Ordering::Relaxed);
        Ok(id)
    }

    /// Release a lease, which removes the key it holds.
    async fn revoke(id: u64) -> Result<(), String> {
        let body = format!("{{\"ID\":{id}}}");
        request("POST", "/v3/lease/revoke", Some(&body))
            .await
            .map(|_| ())
    }

"#;

/// The etcd-only helper that reads a granted lease ID.
const HELPERS: &str = r#"    /// The lease ID from a grant answer. etcd sends `int64` fields as a
    /// number or as a string, so this accepts both.
    fn lease_id(payload: &str) -> Result<u64, String> {
        let answer: serde_json::Value = serde_json::from_str(payload)
            .map_err(|err| format!("cannot read the lease answer: {err}"))?;
        let id = answer
            .get("ID")
            .ok_or_else(|| format!("the lease answer names no lease: {payload}"))?;
        id.as_u64()
            .or_else(|| id.as_str().and_then(|text| text.parse().ok()))
            .ok_or_else(|| format!("the lease answer names no lease: {payload}"))
    }

"#;

/// Standard base64 (RFC 4648) of `input`, which the etcd v3 JSON gateway
/// requires for keys and values.
fn base64(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let mut bytes = [0u8; 3];
        bytes[..chunk.len()].copy_from_slice(chunk);
        let packed = ((bytes[0] as u32) << 16) | ((bytes[1] as u32) << 8) | (bytes[2] as u32);
        // RFC 4648 pads a 1-byte chunk to two data characters, a 2-byte
        // chunk to three, and a 3-byte chunk to four.
        for (index, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            if index > chunk.len() {
                out.push('=');
            } else {
                out.push(TABLE[(packed >> shift) as usize & 63] as char);
            }
        }
    }
    out
}

#[cfg(test)]
pub(super) mod tests {
    use super::base64;

    #[test]
    fn base64_matches_the_rfc_4648_vectors() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64(b"\xff\x00\x7f"), "/wB/");
    }
}
