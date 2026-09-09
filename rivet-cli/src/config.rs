//! `rivet.toml` project configuration.
//!
//! Phase 0 reads the development endpoint and the project name. Extra
//! sections (gauntlet, environments, rust_native_features, ...) parse as
//! noise and are ignored, so the file format can grow ahead of the engine.

use std::path::Path;

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
}
