//! `rivet.toml` project configuration.
//!
//! `rivet build` reads this file from the directory that contains app.py.
//! Unknown sections and keys are ignored, so the file format can grow ahead
//! of the engine. The `[gauntlet]` section tunes the compile-time quality
//! rules in [`crate::gauntlet`].

use crate::diagnostic::Severity;
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

/// The parsed `rivet.toml`.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default)]
pub struct RivetConfig {
    pub project: Project,
    pub environments: Environments,
    /// Compile-time quality-rule settings. Defaults apply when the section
    /// is absent.
    pub gauntlet: GauntletConfig,
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
}
