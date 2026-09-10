//! `rivet.toml` project configuration.
//!
//! `rivet build` reads this file from the directory that contains app.py.
//! Unknown sections and keys are ignored, so the file format can grow ahead
//! of the engine. The `[gauntlet]` section tunes the compile-time quality
//! rules in [`crate::gauntlet`].

use crate::diagnostic::Severity;
use std::collections::BTreeMap;
use std::path::Path;

/// The `[gauntlet]` section: thresholds and per-rule outcomes.
///
/// Defaults keep a minimal project green: `max_complexity` 8, stories
/// required, strict type checking on, duplicate handlers blocked, and dead
/// code reported as a warning. A project tunes a rule by lowering its
/// threshold, setting a flag false, or choosing `warn` as the outcome.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default)]
pub struct GauntletConfig {
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

impl Default for GauntletConfig {
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

/// The parsed `rivet.toml`.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default)]
pub struct RivetConfig {
    pub project: Project,
    pub environments: Environments,
    /// Compile-time quality-rule settings. Defaults apply when the section
    /// is absent.
    pub gauntlet: GauntletConfig,
    /// Plugins composed into the generated app at compile time, keyed by
    /// plugin name. An empty table means the app has no plugins.
    pub plugins: BTreeMap<String, PluginConfig>,
    /// The transport the generated app uses to reach its service layer.
    pub transport: Transport,
    /// The production frontend build the binary embeds.
    pub frontend: Frontend,
    /// The built-in route-table panel.
    pub admin: Admin,
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
mod tests {
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
}
