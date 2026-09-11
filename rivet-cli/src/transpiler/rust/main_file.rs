//! The generated `main.rs`: its blocks, its manifest, and its assembly.
//!
//! [`generate_project`](super::generate_project) renders every part of the
//! crate and hands the parts to [`assemble`], which decides the order they
//! emit in. The manifest and the assembly live here so the shape of the
//! emitted binary is one concern, not a tail on the expression renderer.

use super::ResolvedPlugin;
use crate::config::TransportMode;
use crate::diagnostic::Diagnostic;
use rivet_core::ir::ServiceBlueprint;

/// The generated manifest: the axum stack, the transport's dependencies, the
/// embedded-asset stack, and one dependency per plugin.
///
/// The gRPC transport pulls `tonic` without `prost` or generated code, so the
/// crate builds on a machine with only rustc and cargo.
pub(super) fn render_manifest(
    package_name: &str,
    plugins: &[ResolvedPlugin],
    mode: TransportMode,
    assets: bool,
    discovery: bool,
) -> String {
    // Registration needs the timer and a signal handler; the client needs
    // the async I/O traits. A project without discovery pays for none of it.
    let mut tokio = vec!["macros", "rt-multi-thread", "net"];
    if discovery {
        tokio.extend(["signal", "time", "io-util"]);
    }
    let tokio = tokio
        .iter()
        .map(|feature| format!("\"{feature}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let transport_section = match mode {
        TransportMode::InProcess => String::new(),
        TransportMode::Grpc => String::from(
            "\n# grpc transport: hand-written tonic service, no protoc and no prost.\ntonic = { version = \"0.12\", default-features = false, features = [\"transport\"] }\ntokio-stream = { version = \"0.1\", features = [\"net\"] }\ntower = \"0.4\"\nhttp = \"1\"\nhttp-body = \"1\"\nbytes = \"1\"\n",
        ),
    };
    let assets_section = if assets {
        String::from(
            "\n# The embedded frontend build (pillar 03): `debug-embed` keeps the assets in debug binaries too, so no build reads them from disk, and `compression-br` compresses every response on the wire.\nrust-embed = { version = \"8\", features = [\"mime-guess\", \"debug-embed\"] }\ntower-http = { version = \"0.6\", features = [\"compression-br\"] }\npercent-encoding = \"2\"\n",
        )
    } else {
        String::new()
    };
    let mut dependencies = String::new();
    for plugin in plugins {
        dependencies.push_str(&plugin.dependency);
        dependencies.push('\n');
    }
    let plugin_section = if plugins.is_empty() {
        String::new()
    } else {
        format!("\n# Plugins, composed at compile time by `rivet build`.\n{dependencies}")
    };
    format!(
        r#"[workspace]

[package]
name = "{package_name}"
version = "0.1.0"
edition = "2024"

[dependencies]
axum = "0.8"
tokio = {{ version = "1", features = [{tokio}] }}
serde = {{ version = "1", features = ["derive"] }}
serde_json = "1"
tracing = "0.1"
tracing-subscriber = {{ version = "0.3", features = ["env-filter"] }}
{transport_section}{assets_section}{plugin_section}
[profile.release]
strip = true
"#
    )
}

/// The rendered blocks of `main.rs`, in the order the file emits them.
pub(super) struct MainBlocks<'a> {
    pub(super) structs: &'a str,
    pub(super) service: &'a str,
    pub(super) channel: &'a str,
    pub(super) handlers: &'a str,
    pub(super) router: &'a str,
    /// The `mod assets` block, or empty when the app embeds no assets.
    pub(super) assets: &'a str,
    /// The `mod admin` block, or empty when the panel is off.
    pub(super) admin: &'a str,
    /// The `mod discovery` block, or empty when the app registers nowhere.
    pub(super) discovery: &'a str,
}

/// Bundled main-file inputs so `assemble` stays under the argument-count
/// threshold.
pub(super) struct MainParts {
    pub(super) routing: String,
    pub(super) host: String,
    pub(super) port: u16,
    /// The plugin install calls, already framed with blank lines, or empty
    /// when the project configures no plugins.
    pub(super) plugins: String,
    /// The statements that start the service channel, or empty in
    /// `in_process` mode.
    pub(super) channel_setup: String,
    /// The expression the router takes as state.
    pub(super) channel_state: String,
    /// The router's asset-fallback line, or empty when the app embeds no
    /// assets.
    pub(super) assets_fallback: String,
    /// The compression layer applied after the plugins install, or empty
    /// when the app embeds no assets.
    pub(super) assets_layer: String,
    /// The router lines that mount the admin panel, or empty when the panel
    /// is off.
    pub(super) admin_routes: String,
    /// The statements that join the service registry, or empty when the app
    /// registers nowhere.
    pub(super) discovery_register: String,
    /// The statements that serve and leave the registry. Holds the plain
    /// serve statement when the app registers nowhere.
    pub(super) discovery_serve: String,
}

/// The axum routing functions the blueprint actually uses, in first-use
/// order, so the generated `use` list has no unused imports. The admin panel
/// adds `GET` when the blueprint declares no `GET` route of its own.
pub(super) fn used_router_fns(blueprint: &ServiceBlueprint, admin: bool) -> Vec<&'static str> {
    let mut seen: Vec<&'static str> = Vec::new();
    for route in &blueprint.routes {
        let name = route.method.axum_router_fn();
        if !seen.contains(&name) {
            seen.push(name);
        }
    }
    if admin && !seen.contains(&"get") {
        seen.push("get");
    }
    seen
}

/// Assemble the final `main.rs` from its parts.
pub(super) fn assemble(blocks: &MainBlocks<'_>, parts: &MainParts) -> Result<String, Diagnostic> {
    // The helpers are always emitted and annotated so unused ones do not
    // warn in the generated crate. The two the service layer calls are
    // shared with the WebAssembly target; `channel_error` is native-only,
    // because it names the axum status type.
    let helpers = format!(
        "{}{}",
        super::helpers::SHARED,
        super::helpers::CHANNEL_ERROR
    );
    Ok(format!(
        r#"// Generated by rivet {version}. Do not edit; run `rivet build` again.
use axum::{{extract::{{Json, State}}, routing::{{{routing}}}, Router}};

{structs}{helpers}{service}{channel}{handlers}{admin}{discovery}{assets}
#[tokio::main]
async fn main() {{
    tracing_subscriber::fmt::init();

    let host = std::env::var("HOST").unwrap_or_else(|_| "{host}".to_string());
    let port: u16 = std::env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or({port});

{channel_setup}    let app = Router::new()
{router}{admin_routes}{assets_fallback}
        .with_state({channel_state});
{plugins}{assets_layer}    let addr = format!("{{host}}:{{port}}");
    let listener = tokio::net::TcpListener::bind(&addr).await.expect("failed to bind address");
    println!("rivet app listening on http://{{addr}}");
{discovery_register}{discovery_serve}
}}
"#,
        version = env!("CARGO_PKG_VERSION"),
        routing = parts.routing,
        structs = blocks.structs,
        helpers = helpers,
        service = blocks.service,
        channel = blocks.channel,
        handlers = blocks.handlers,
        router = blocks.router,
        assets = blocks.assets,
        admin = blocks.admin,
        discovery = blocks.discovery,
        discovery_register = parts.discovery_register,
        discovery_serve = parts.discovery_serve,
        admin_routes = parts.admin_routes,
        assets_fallback = parts.assets_fallback,
        assets_layer = parts.assets_layer,
        plugins = parts.plugins,
        channel_setup = parts.channel_setup,
        channel_state = parts.channel_state,
        host = parts.host,
        port = parts.port,
    ))
}
