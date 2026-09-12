//! `rivet.toml` project configuration.
//!
//! `rivet build` reads this file from the directory that contains app.py.
//! Unknown sections and keys are ignored, so the file format can grow ahead
//! of the engine. The `[verifier]` section tunes the compile-time quality
//! rules in [`crate::verifier`].

use crate::diagnostic::Severity;
use std::collections::BTreeMap;
use std::path::Path;

/// The `[verifier]` section: thresholds and per-rule outcomes.
///
/// Defaults keep a minimal project green: `max_complexity` 8, stories
/// required, strict type checking on, duplicate handlers blocked, and dead
/// code reported as a warning. A project tunes a rule by lowering its
/// threshold, setting a flag false, or choosing `warn` as the outcome.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default)]
pub struct VerifierConfig {
    /// Maximum cyclomatic complexity for a handler or DTO class.
    pub max_complexity: usize,
    /// Require at least one story ID on every route (pillar 05).
    pub stories_required: bool,
    /// Reject module constructs that cannot reach the generated server.
    pub strict_type_checking: bool,
    /// Outcome for handlers that share an implementation.
    pub duplicate_code: Severity,
    /// Outcome for helpers and DTO classes nothing references.
    pub dead_code: Severity,
}

impl Default for VerifierConfig {
    fn default() -> Self {
        Self {
            max_complexity: 8,
            stories_required: true,
            strict_type_checking: true,
            duplicate_code: Severity::Blocker,
            dead_code: Severity::Warning,
        }
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default)]
pub struct Project {
    pub name: String,
}

impl Default for Project {
    fn default() -> Self {
        Self {
            name: "app".to_string(),
        }
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default)]
pub struct Development {
    pub host: String,
    pub port: u16,
}

impl Default for Development {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 3000,
        }
    }
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default)]
pub struct Environments {
    pub development: Development,
}

/// One plugin declaration from `[plugins.<name>]`.
///
/// The table key is the plugin name (`auth-token`). The crate name defaults
/// to `rivet-plugin-<name>`, and the plugin resolves either from a local
/// `path` or from a registry `version`.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default)]
pub struct PluginConfig {
    /// The plugin crate name; defaults to `rivet-plugin-<name>`.
    #[serde(rename = "crate")]
    pub crate_name: Option<String>,
    /// The plugin crate directory, relative to the project directory.
    pub path: Option<String>,
    /// The registry version, used when `path` is absent.
    pub version: Option<String>,
}

impl PluginConfig {
    /// The crate that implements this plugin.
    pub fn crate_name(&self, plugin: &str) -> String {
        self.crate_name
            .clone()
            .unwrap_or_else(|| format!("rivet-plugin-{plugin}"))
    }
}

/// The `[transport]` section: how the generated app reaches its own service
/// layer (pillar 02).
///
/// `in_process` compiles a direct, monomorphized call into the binary — the
/// monolith. `grpc` compiles a call over a gRPC channel and makes the binary
/// serve that channel on `grpc_port` too, so a second process running the
/// same blueprint can be the service.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default)]
pub struct Transport {
    pub mode: TransportMode,
    /// The port the service channel serves on when `mode = "grpc"`.
    pub grpc_port: u16,
}

impl Default for Transport {
    fn default() -> Self {
        Self {
            mode: TransportMode::InProcess,
            grpc_port: 50051,
        }
    }
}

/// The topology the generated app runs in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransportMode {
    /// One process: route logic is a direct call.
    InProcess,
    /// The app calls its own logic over gRPC and serves the channel.
    Grpc,
}

/// The `[frontend]` section: the production build `rivet build` embeds.
///
/// `rivet dev` proxies to a frontend dev server; the built binary has no
/// sidecar files, so `rivet build` compiles the directory `dist` names into
/// the binary and serves it from the router's fallback (pillar 03). A
/// project with no `[frontend]` section embeds nothing.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default)]
pub struct Frontend {
    /// The production build directory, relative to the project directory.
    /// Absent means the binary embeds no assets.
    pub dist: Option<String>,
    /// Answer a path that matches no asset with `index.html`, so a
    /// client-side router owns the path. A directory path resolves to its
    /// own `index.html` first.
    pub spa: bool,
}

impl Default for Frontend {
    fn default() -> Self {
        Self {
            dist: None,
            spa: true,
        }
    }
}

/// The `[admin]` section: the built-in route-table panel (phase 4).
///
/// `enabled = true` mounts `GET /__rivet/routes` and `GET /__rivet/` on the
/// generated app. A project with no `[admin]` section serves neither.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default)]
pub struct Admin {
    /// Serve the route table and the panel.
    pub enabled: bool,
}

/// The `[rust_native_features]` section: the four Rust capabilities a
/// project can opt into (phase 5).
///
/// Each flag is `false` by default, and a flag the generator does not
/// implement stays `false`: the section advertises what the generator does,
/// and a config must never over-claim. `const_generics` landed first — it
/// renders `List[T, N]` as a fixed-size array instead of failing with
/// `E2003` — and `zero_copy_deserialization` followed it, rendering a
/// `borrowed[str]` field as a `&str` slice of the request body.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default)]
pub struct RustNativeFeatures {
    /// Render `List[T, N]` as `[T; N]`.
    pub const_generics: bool,
    /// Borrow request bodies through `#[serde(borrow)]` instead of copying.
    pub zero_copy_deserialization: bool,
    /// Release pooled connections automatically at handler exit.
    pub raii_connections: bool,
    /// Require an authenticated request type at compile time on a protected
    /// route.
    pub compile_time_rbac: bool,
}

impl RustNativeFeatures {
    /// The first flag the generator does not implement, when the project set
    /// one.
    ///
    /// A set-but-unimplemented flag is a blocker rather than a silent
    /// no-op: the section advertises what the generator does, and a config
    /// that claims a capability the build does not apply would mislead every
    /// reader of that file.
    pub fn unimplemented(&self) -> Option<&'static str> {
        [
            ("raii_connections", self.raii_connections),
            ("compile_time_rbac", self.compile_time_rbac),
        ]
        .into_iter()
        .find(|(_, set)| *set)
        .map(|(name, _)| name)
    }
}

/// The `[discovery]` section: the service registry the generated app joins
/// (phase 4, pillar 02).
///
/// `backend` selects the registry and turns registration on; a project with
/// no `[discovery]` section registers nowhere. The generated app posts its
/// name and port to the registry at startup and removes itself on shutdown.
/// A registry that does not answer only warns: the server starts either way.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default)]
pub struct Discovery {
    /// The registry backend; absent means the app registers nowhere.
    pub backend: Option<DiscoveryBackend>,
    /// The registry's HTTP endpoint. Defaults to the backend's local port.
    pub url: Option<String>,
    /// The name the app registers under. Defaults to the project name.
    pub service_name: Option<String>,
    /// The port the app advertises. Defaults to the development port.
    pub service_port: Option<u16>,
}

impl Discovery {
    /// The registry endpoint, with the backend's default when the project
    /// configures none. `None` when the app registers nowhere.
    pub fn url(&self) -> Option<String> {
        let backend = self.backend?;
        Some(
            self.url
                .clone()
                .unwrap_or_else(|| backend.default_url().to_string()),
        )
    }

    /// The service name the app registers under.
    pub fn service_name(&self, project: &str) -> String {
        self.service_name
            .clone()
            .unwrap_or_else(|| project.to_string())
    }
}

/// The registry the generated app registers itself with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryBackend {
    /// The Consul agent's HTTP API.
    Consul,
    /// The etcd v3 JSON gateway.
    Etcd,
}

impl DiscoveryBackend {
    /// The endpoint a local registry answers on.
    pub fn default_url(self) -> &'static str {
        match self {
            DiscoveryBackend::Consul => "http://127.0.0.1:8500",
            DiscoveryBackend::Etcd => "http://127.0.0.1:2379",
        }
    }
}

/// The parsed `rivet.toml`.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default)]
pub struct RivetConfig {
    pub project: Project,
    pub environments: Environments,
    /// Compile-time quality-rule settings. Defaults apply when the section
    /// is absent.
    pub verifier: VerifierConfig,
    /// Plugins composed into the generated app at compile time, keyed by
    /// plugin name. An empty table means the app has no plugins.
    pub plugins: BTreeMap<String, PluginConfig>,
    /// The transport the generated app uses to reach its service layer.
    pub transport: Transport,
    /// The production frontend build the binary embeds.
    pub frontend: Frontend,
    /// The built-in route-table panel.
    pub admin: Admin,
    /// The service registry the generated app joins.
    pub discovery: Discovery,
    /// The Rust capabilities this project opts into.
    pub rust_native_features: RustNativeFeatures,
}

impl RivetConfig {
    /// Load `rivet.toml` from a project directory. A missing file is not an
    /// error: defaults apply. A malformed file is.
    pub fn load(project_dir: &Path) -> Result<RivetConfig, String> {
        let path = project_dir.join("rivet.toml");
        let raw = match std::fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Ok(RivetConfig::default());
            }
            Err(err) => return Err(format!("failed to read {}: {err}", path.display())),
        };
        toml::from_str(&raw).map_err(|err| format!("failed to parse {}: {err}", path.display()))
    }
}

#[cfg(test)]
mod tests;
