//! `rivet dev`: one origin for the Rust backend and the frontend dev server.
//!
//! The command detects the frontend from its config file, builds and starts
//! the generated backend on a private port, then proxies on the project's
//! configured port. The backend keeps the blueprint's own route paths, and
//! `/api/*` is the proxy's escape hatch to it; every other path reaches the
//! frontend dev server, whose HMR socket the proxy tunnels. An upstream that
//! does not answer returns `502` with the reason, never a hang.

use crate::commands::build::BuildTarget;
use crate::config::RivetConfig;
use crate::diagnostic::Diagnostic;
use crate::parser::python::parse_python_file;
use crate::transpiler::rust::crate_name;
use axum::Router;
use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderName, StatusCode, header};
use axum::response::{IntoResponse, Response};
use routing::{upstream_for, upstream_url};
use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::Arc;
use std::time::Duration;

pub(crate) mod routing;
pub(crate) mod tunnel;

/// The largest request body the proxy buffers.
const MAX_BODY: usize = 64 * 1024 * 1024;

/// How long an upstream has to answer before the proxy reports `502`.
const UPSTREAM_TIMEOUT: Duration = Duration::from_secs(30);

/// How long the generated backend has to start listening.
const BACKEND_TIMEOUT: Duration = Duration::from_secs(30);

/// Error code for a development topology that cannot start.
const CODE_DEV: &str = "E3018";

/// A detected frontend dev server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Frontend {
    /// The framework name, for example `vite`.
    pub framework: &'static str,
    /// The port the framework's dev server listens on by default.
    pub default_port: u16,
}

/// The config-file stems that identify a frontend, with its default dev port.
const FRONTENDS: &[(&str, &str, u16)] = &[
    ("vite.config", "vite", 5173),
    ("rsbuild.config", "rsbuild", 3000),
    ("next.config", "next", 3000),
    ("webpack.config", "webpack", 8080),
];

/// The JavaScript and TypeScript config extensions Rivet looks for.
const CONFIG_EXTENSIONS: &[&str] = &["js", "mjs", "cjs", "ts", "mts", "cts"];

/// Detect the frontend from its config file in `project_dir`.
///
/// Returns `None` when the directory holds no known frontend config.
pub fn detect_frontend(project_dir: &Path) -> Option<Frontend> {
    for (stem, framework, default_port) in FRONTENDS {
        for extension in CONFIG_EXTENSIONS {
            if project_dir.join(format!("{stem}.{extension}")).is_file() {
                return Some(Frontend {
                    framework,
                    default_port: *default_port,
                });
            }
        }
    }
    None
}

/// The development topology: where each kind of path goes.
pub struct DevTopology {
    /// The Rust backend origin, for example `http://127.0.0.1:3001`.
    pub backend: String,
    /// The frontend dev server origin, when the project has a frontend.
    pub frontend: Option<String>,
    /// The route paths the blueprint declares, so the backend keeps them.
    pub routes: Vec<String>,
}

/// Build and run the development topology until the process stops.
pub fn run_dev(
    app_file: &Path,
    frontend_port: Option<u16>,
    backend_port: Option<u16>,
) -> Result<(), Vec<Diagnostic>> {
    let project_dir = project_dir_for(app_file);
    let config = RivetConfig::load(&project_dir).map_err(|message| vec![dev_error(message)])?;
    let frontend = detect_frontend(&project_dir);
    let frontend_port = frontend_port.or_else(|| frontend.map(|frontend| frontend.default_port));

    let host = config.environments.development.host.clone();
    let public_port = config.environments.development.port;
    let backend_port = backend_port.unwrap_or_else(|| public_port.saturating_add(1));

    if let Some(port) = frontend_port
        && port == public_port
    {
        return Err(vec![dev_error(format!(
            "the frontend dev port {port} is the port the proxy listens on; give the frontend another port with `--frontend-port`"
        ))]);
    }

    // The proxy routes the blueprint's own paths to the backend, so the
    // module must parse before the topology starts.
    let routes = route_paths(app_file)?;

    match frontend {
        Some(frontend) => println!(
            "Detected {}: the backend keeps {} route(s) and {}/*, port {frontend_port:?} serves the rest",
            frontend.framework,
            routes.len(),
            routing::API_PREFIX
        ),
        None => println!(
            "No frontend config found: serving every path from the backend ({} route(s))",
            routes.len()
        ),
    }

    crate::commands::build::run_build(app_file, BuildTarget::Native)?;

    let binary = project_dir
        .join("generated")
        .join("target")
        .join("release")
        .join(crate_name(&config.project.name));

    let runtime = tokio::runtime::Runtime::new()
        .map_err(|err| vec![dev_error(format!("cannot start the tokio runtime: {err}"))])?;
    runtime.block_on(async move {
        // Take the public port first: the spawned backend does not die with
        // this process, so a later failure must not leave it running.
        let listener = tokio::net::TcpListener::bind((host.as_str(), public_port))
            .await
            .map_err(|err| {
                vec![dev_error(format!(
                    "cannot listen on {host}:{public_port}: {err}"
                ))]
            })?;
        let mut backend = spawn_backend(&binary, &host, backend_port)?;
        if let Err(diagnostics) = wait_for_backend(&host, backend_port).await {
            let _ = backend.kill();
            let _ = backend.wait();
            return Err(diagnostics);
        }
        println!("rivet dev listening on http://{host}:{public_port}");
        let topology = DevTopology {
            backend: format!("http://{host}:{backend_port}"),
            frontend: frontend_port.map(|port| format!("http://{host}:{port}")),
            routes,
        };
        let result = tokio::select! {
            served = serve_proxy(listener, topology) => served
                .map_err(|err| vec![dev_error(format!("the dev proxy stopped: {err}"))]),
            _ = tokio::signal::ctrl_c() => Ok(()),
        };
        let _ = backend.kill();
        let _ = backend.wait();
        result
    })
}

/// The route paths the blueprint declares.
fn route_paths(app_file: &Path) -> Result<Vec<String>, Vec<Diagnostic>> {
    let module = parse_python_file(app_file).map_err(|diagnostic| vec![diagnostic])?;
    Ok(module
        .blueprint
        .routes
        .iter()
        .map(|route| route.path.clone())
        .collect())
}

/// Start the generated backend on the private port.
fn spawn_backend(binary: &Path, host: &str, port: u16) -> Result<Child, Vec<Diagnostic>> {
    std::process::Command::new(binary)
        .env("HOST", host)
        .env("PORT", port.to_string())
        .spawn()
        .map_err(|err| {
            vec![dev_error(format!(
                "cannot start {}: {err}",
                binary.display()
            ))]
        })
}

/// Wait until the generated backend accepts connections.
async fn wait_for_backend(host: &str, port: u16) -> Result<(), Vec<Diagnostic>> {
    let deadline = tokio::time::Instant::now() + BACKEND_TIMEOUT;
    while tokio::time::Instant::now() < deadline {
        if tokio::net::TcpStream::connect((host, port)).await.is_ok() {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Err(vec![dev_error(format!(
        "the generated backend never listened on {host}:{port}"
    ))])
}

/// Serve the proxy on `listener` until the process stops.
pub async fn serve_proxy(
    listener: tokio::net::TcpListener,
    topology: DevTopology,
) -> Result<(), std::io::Error> {
    let state = Arc::new(DevState {
        // Building a plain HTTP client cannot fail; the default client is
        // the fallback rather than an error path.
        client: reqwest::Client::builder()
            .timeout(UPSTREAM_TIMEOUT)
            .build()
            .unwrap_or_default(),
        topology,
    });
    let app = Router::new().fallback(proxy).with_state(state);
    axum::serve(listener, app).await
}

/// The proxy's shared state: one HTTP client and the routing table.
struct DevState {
    client: reqwest::Client,
    topology: DevTopology,
}

/// Forward one request to the upstream that owns its path.
///
/// An upgrade request goes to the frontend dev server over a tunnel instead:
/// its HMR socket connects to the page origin, which is this proxy, and no
/// HTTP client can carry it.
async fn proxy(State(state): State<Arc<DevState>>, request: Request) -> Response {
    let topology = &state.topology;
    let origin = upstream_for(topology, request.uri().path());
    if tunnel::is_upgrade_request(request.headers()) {
        return tunnel::tunnel(topology, &origin, request).await;
    }

    let (parts, body) = request.into_parts();
    let target = upstream_url(topology, &origin, &parts.uri);
    let body = match axum::body::to_bytes(body, MAX_BODY).await {
        Ok(body) => body,
        Err(err) => return bad_gateway(&format!("cannot read the request body: {err}")),
    };

    let outbound = state
        .client
        .request(parts.method.clone(), target)
        .headers(forward_headers(&parts.headers))
        .body(body);
    match outbound.send().await {
        Ok(response) => forward_response(response).await,
        Err(err) => bad_gateway(&format!("{origin} did not answer: {err}")),
    }
}

/// The headers to send upstream: the request's headers minus the hop-by-hop
/// ones, whose values belong to this connection only.
fn forward_headers(headers: &HeaderMap) -> HeaderMap {
    let mut out = HeaderMap::new();
    for (name, value) in headers {
        if is_hop_by_hop(name) {
            continue;
        }
        out.append(name, value.clone());
    }
    out
}

/// Whether a header describes this connection rather than the message.
fn is_hop_by_hop(name: &HeaderName) -> bool {
    matches!(
        name.as_str(),
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
            | "host"
            | "content-length"
    )
}

/// Rebuild the upstream response for the client.
async fn forward_response(response: reqwest::Response) -> Response {
    let status = response.status();
    let headers = response.headers().clone();
    let body = match response.bytes().await {
        Ok(body) => body,
        Err(err) => return bad_gateway(&format!("cannot read the upstream response: {err}")),
    };
    let mut out = Response::new(Body::from(body));
    *out.status_mut() = status;
    for (name, value) in &headers {
        if is_hop_by_hop(name) {
            continue;
        }
        out.headers_mut().append(name.clone(), value.clone());
    }
    out
}

/// A `502` response that names the upstream problem.
fn bad_gateway(detail: &str) -> Response {
    eprintln!("rivet dev: {detail}");
    (
        StatusCode::BAD_GATEWAY,
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        format!("rivet dev: {detail}\n"),
    )
        .into_response()
}

/// The project directory that owns `rivet.toml`.
fn project_dir_for(app_file: &Path) -> PathBuf {
    app_file
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn dev_error(detail: String) -> Diagnostic {
    Diagnostic::blocker(
        CODE_DEV,
        format!("cannot start the development server: {detail}"),
        format!("fix the development topology and run `rivet dev` again: {detail}"),
    )
    .located("rivet.toml", 1)
}

#[cfg(test)]
mod tests;
