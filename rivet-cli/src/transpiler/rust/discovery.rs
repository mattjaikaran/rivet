//! Service discovery: the generated app joins a registry at startup.
//!
//! `[discovery] backend = "consul" | "etcd"` makes the generated app press
//! its name and port into a service registry (phase 4, pillar 02). Consul
//! uses the agent's own service API; etcd uses the v3 JSON gateway with a
//! lease, so a crashed app drops out of the registry on its own.
//!
//! Registration happens once, at startup, so the generated crate carries a
//! small hand-written HTTP/1.1 client instead of an HTTP stack: it speaks
//! plain `http://`, opens one connection per request, and closes it. No
//! dependency is added, and nothing in the request path changes.
//!
//! A registry that does not answer only prints a warning. A discovery
//! outage must not stop the app from serving, so `register` never fails the
//! process.

use crate::config::{DiscoveryBackend, RivetConfig};
use crate::diagnostic::Diagnostic;
use serde_json::Value;

mod etcd;

#[cfg(test)]
mod tests;

/// The generated discovery wiring for `main.rs`.
///
/// Every field is empty when the project configures no backend, except
/// [`serve`](Wiring::serve), which then holds the plain serve statement.
#[derive(Debug)]
pub(super) struct Wiring {
    /// The `mod discovery` block.
    pub(super) module: String,
    /// The statements that join the registry, before the app serves.
    pub(super) register: String,
    /// The statements that serve and then leave the registry.
    pub(super) serve: String,
}

/// The serve statement a project without discovery keeps, unchanged.
const PLAIN_SERVE: &str = "    axum::serve(listener, app).await.expect(\"server error\");\n";

/// The statements that join the registry before the app serves.
const REGISTER: &str = "\n    // Join the service registry. A registry that does not answer must\n    // not stop the app, so a failure only warns.\n    if let Err(err) = discovery::register().await {\n        eprintln!(\"service registration failed: {err}\");\n    }\n\n";

/// The statements that serve until Ctrl-C and then leave the registry.
const SERVE: &str = "    let shutdown = async {\n        let _ = tokio::signal::ctrl_c().await;\n    };\n    axum::serve(listener, app)\n        .with_graceful_shutdown(shutdown)\n        .await\n        .expect(\"server error\");\n\n    // Leave the registry on the way out.\n    if let Err(err) = discovery::deregister().await {\n        eprintln!(\"service deregistration failed: {err}\");\n    }\n";

/// Render the generated discovery wiring for the project's configuration.
///
/// Returns an [`E2005`](Diagnostic::blocker) diagnostic when the service
/// name cannot go into a registry URL or key.
pub(super) fn render(config: &RivetConfig) -> Result<Wiring, Diagnostic> {
    let Some(backend) = config.discovery.backend else {
        return Ok(Wiring {
            module: String::new(),
            register: String::new(),
            serve: PLAIN_SERVE.to_string(),
        });
    };
    let name = config.discovery.service_name(&config.project.name);
    if let Some(problem) = name_problem(&name) {
        return Err(service_name_diagnostic(&name, problem));
    }
    let port = config
        .discovery
        .service_port
        .unwrap_or(config.environments.development.port);
    let url = config
        .discovery
        .url()
        .unwrap_or_else(|| backend.default_url().to_string());
    let (register, deregister, helpers) = match backend {
        DiscoveryBackend::Consul => consul_parts(&name, port),
        DiscoveryBackend::Etcd => etcd::parts(&name, port),
    };

    Ok(Wiring {
        module: MODULE
            .replace("@@SERVICE@@", &super::rust_str(&name))
            .replace("@@PORT@@", &port.to_string())
            .replace("@@REGISTRY@@", &super::rust_str(&url))
            .replace("@@REGISTER@@", &register)
            .replace("@@DEREGISTER@@", &deregister)
            .replace("@@HELPERS@@", &helpers),
        register: REGISTER.to_string(),
        serve: SERVE.to_string(),
    })
}

/// The Consul registration, deregistration, and helpers for one service.
fn consul_parts(name: &str, port: u16) -> (String, String, String) {
    let mut body = serde_json::Map::new();
    body.insert("ID".into(), Value::String(name.to_string()));
    body.insert("Name".into(), Value::String(name.to_string()));
    body.insert("Port".into(), Value::from(port));
    let body = Value::Object(body).to_string();
    let path = format!("{CONSUL_DEREGISTER_PATH}{name}");

    let register = format!(
        "    /// The Consul agent service registration: this app's name and port.\n    const REGISTER_BODY: &str = {};\n\n    /// Register this service with the Consul agent.\n    pub(super) async fn register() -> Result<String, String> {{\n        request(\"PUT\", \"{CONSUL_REGISTER_PATH}\", Some(REGISTER_BODY)).await?;\n        Ok(format!(\"registered {{SERVICE_NAME}} on port {{SERVICE_PORT}} with consul\"))\n    }}\n",
        super::rust_str(&body),
    );
    let deregister = format!(
        "    /// Remove this service from the Consul agent.\n    pub(super) async fn deregister() -> Result<(), String> {{\n        request(\"PUT\", {}, None).await.map(|_| ())\n    }}\n",
        super::rust_str(&path),
    );
    (register, deregister, String::new())
}

/// The Consul agent endpoint that registers a service.
const CONSUL_REGISTER_PATH: &str = "/v1/agent/service/register";

/// The Consul agent endpoint that removes one, with the name appended.
const CONSUL_DEREGISTER_PATH: &str = "/v1/agent/service/deregister/";

/// The discovery module every registering app carries.
const MODULE: &str = r#"/// Service registration for the configured registry (phase 4, pillar 02).
///
/// The app joins the registry once, at startup, and leaves it on the way
/// out. A registry that does not answer only warns: the server starts
/// either way, because a discovery outage must not take the app down.
mod discovery {
    /// The name this app registers under.
    const SERVICE_NAME: &str = @@SERVICE@@;

    /// The port this app advertises.
    const SERVICE_PORT: u16 = @@PORT@@;

    /// The registry's HTTP endpoint.
    const REGISTRY: &str = @@REGISTRY@@;

    /// The largest registry answer this client reads.
    const MAX_ANSWER: u64 = 1 << 20;

    /// How long one registry request may take, connect included.
    ///
    /// A registry that accepts a connection and then never answers must not
    /// hold startup: the app serves either way, so a request that runs past
    /// this deadline fails instead of hanging.
    const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

@@REGISTER@@
@@DEREGISTER@@
@@HELPERS@@    /// Send one request to the registry and return its response body,
    /// within [`REQUEST_TIMEOUT`].
    async fn request(method: &str, path: &str, body: Option<&str>) -> Result<String, String> {
        match tokio::time::timeout(REQUEST_TIMEOUT, exchange(method, path, body)).await {
            Ok(answer) => answer,
            Err(_) => Err(format!(
                "the registry at {REGISTRY} did not answer within {}s",
                REQUEST_TIMEOUT.as_secs()
            )),
        }
    }

    /// Send one request to the registry and return its response body.
    ///
    /// The client speaks plain HTTP/1.1 over one connection per request:
    /// registration happens once at startup, so a pool would cost more than
    /// it saves. An `https://` endpoint needs a TLS terminator in front of
    /// the registry; this client reports it instead of pretending.
    async fn exchange(method: &str, path: &str, body: Option<&str>) -> Result<String, String> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let (authority, base) = registry_address()?;
        let payload = body.unwrap_or_default();
        let head = format!(
            "{method} {base}{path} HTTP/1.1\r\nhost: {authority}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
            payload.len()
        );
        let mut stream = tokio::net::TcpStream::connect(&authority)
            .await
            .map_err(|err| format!("cannot reach the registry at {authority}: {err}"))?;
        stream
            .write_all(head.as_bytes())
            .await
            .map_err(|err| format!("cannot send to the registry at {authority}: {err}"))?;
        stream
            .write_all(payload.as_bytes())
            .await
            .map_err(|err| format!("cannot send to the registry at {authority}: {err}"))?;

        let mut raw = Vec::new();
        if stream
            .take(MAX_ANSWER)
            .read_to_end(&mut raw)
            .await
            .map_err(|err| format!("cannot read the registry answer: {err}"))?
            == MAX_ANSWER as usize
        {
            return Err("the registry answer is larger than 1 MiB".to_string());
        }
        let split = raw
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .ok_or_else(|| "the registry answered no HTTP header".to_string())?;
        let head = String::from_utf8_lossy(&raw[..split]).into_owned();
        let body = &raw[split + 4..];
        let status = head
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|code| code.parse::<u16>().ok())
            .ok_or_else(|| "the registry answered no HTTP status".to_string())?;
        if !(200..300).contains(&status) {
            return Err(format!(
                "the registry answered {status}: {}",
                String::from_utf8_lossy(body)
            ));
        }
        let answer = if head.to_ascii_lowercase().contains("transfer-encoding: chunked") {
            dechunk(body)?
        } else {
            body.to_vec()
        };
        Ok(String::from_utf8_lossy(&answer).into_owned())
    }

    /// The registry's `host:port` and its path prefix, from `REGISTRY`.
    fn registry_address() -> Result<(String, String), String> {
        let rest = REGISTRY.strip_prefix("http://").ok_or_else(|| {
            format!("this client speaks plain HTTP, so the registry URL must start with http://: {REGISTRY}")
        })?;
        let (authority, base) = match rest.split_once('/') {
            Some((authority, base)) => (authority, format!("/{}", base.trim_end_matches('/'))),
            None => (rest, String::new()),
        };
        if authority.is_empty() {
            return Err(format!("the registry URL names no host: {REGISTRY}"));
        }
        Ok((authority.to_string(), base))
    }

    /// Decode an HTTP chunked body.
    fn dechunk(body: &[u8]) -> Result<Vec<u8>, String> {
        let mut decoded = Vec::new();
        let mut rest = body;
        loop {
            let end = rest
                .windows(2)
                .position(|window| window == b"\r\n")
                .ok_or_else(|| "the registry sent a broken chunk header".to_string())?;
            let header = String::from_utf8_lossy(&rest[..end]);
            let size = usize::from_str_radix(
                header.trim().split(';').next().unwrap_or_default(),
                16,
            )
            .map_err(|err| format!("the registry sent a broken chunk size: {err}"))?;
            if size == 0 {
                return Ok(decoded);
            }
            let data = rest
                .get(end + 2..end + 2 + size)
                .ok_or_else(|| "the registry sent a short chunk".to_string())?;
            decoded.extend_from_slice(data);
            rest = rest.get(end + 2 + size..).unwrap_or_default();
        }
    }
}
"#;

/// The reason a service name cannot go into a registry URL or key.
fn name_problem(name: &str) -> Option<&'static str> {
    if name.is_empty() {
        return Some("it is empty");
    }
    if name.len() > 128 {
        return Some("it is longer than 128 characters");
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
    {
        return Some("it holds a character outside letters, digits, dot, dash, and underscore");
    }
    None
}

/// The `E2005` diagnostic for an unusable service name.
fn service_name_diagnostic(name: &str, problem: &str) -> Diagnostic {
    Diagnostic::blocker(
        "E2005",
        format!("`{name}` is not a usable service name: {problem}"),
        "set `[discovery] service_name`, or `[project] name`, to letters, digits, dots, dashes, and underscores (128 characters or fewer), then rerun the command",
    )
    .located("rivet.toml", 1)
}
