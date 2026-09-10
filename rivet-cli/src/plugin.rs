//! Plugin resolution for the generated app.
//!
//! A Rivet plugin is a Rust crate that implements
//! [`rivet_plugin_api::Plugin`] and exports
//! `pub fn install(router: axum::Router) -> axum::Router`. The `[plugins]`
//! section of `rivet.toml` names the plugins a project composes.
//!
//! Resolution happens at generation time and returns one dependency and one
//! call site per plugin, so the generated app looks the plugin up in the
//! compiler, never in a registry: no `dyn`, no name table, no runtime
//! indirection. A plugin that cannot resolve stops the build before any
//! crate is written.

use crate::config::RivetConfig;
use crate::diagnostic::Diagnostic;
use std::path::Path;

/// Error code for a plugin declaration that cannot resolve.
const CODE_PLUGIN: &str = "E3016";

/// One configured plugin, resolved to generated-crate wiring.
#[derive(Debug)]
pub struct ResolvedPlugin {
    /// The `[plugins.<name>]` key.
    pub name: String,
    /// The Rust crate that implements the plugin.
    pub crate_name: String,
    /// The dependency line for the generated `Cargo.toml`.
    pub dependency: String,
}

impl ResolvedPlugin {
    /// The crate name as Rust spells it, for example `rivet_plugin_auth_token`.
    pub fn crate_ident(&self) -> String {
        self.crate_name.replace('-', "_")
    }
}

/// Resolve every configured plugin against the project directory.
///
/// The result is ordered by plugin name so the generated crate is
/// deterministic. A project with no plugins resolves to an empty vector and
/// generates no plugin code.
pub fn resolve(
    config: &RivetConfig,
    project_dir: &Path,
) -> Result<Vec<ResolvedPlugin>, Diagnostic> {
    let mut resolved = Vec::with_capacity(config.plugins.len());
    for (name, plugin) in &config.plugins {
        if !valid_name(name) {
            return Err(unresolvable(format!(
                "plugin name `{name}` is not a crate-name segment"
            )));
        }
        let crate_name = plugin.crate_name(name);
        if !valid_name(&crate_name) {
            return Err(unresolvable(format!(
                "plugin `{name}` resolves to the crate name `{crate_name}`, which is not a crate-name segment"
            )));
        }
        if plugin.version.as_deref() == Some("") {
            return Err(unresolvable(format!(
                "plugin `{name}` sets an empty `version`"
            )));
        }
        let dependency = match plugin.path.as_deref() {
            Some(path) => {
                let dependency_path = path_dependency(project_dir, path).ok_or_else(|| {
                    unresolvable(format!(
                        "plugin `{name}` points at `{path}`, which has no `Cargo.toml`"
                    ))
                })?;
                format!("{crate_name} = {{ path = {dependency_path:?} }}")
            }
            None => match plugin.version.as_deref() {
                Some(version) => format!("{crate_name} = {version:?}"),
                None => {
                    return Err(unresolvable(format!(
                        "plugin `{name}` sets neither `path` nor `version`"
                    )));
                }
            },
        };
        resolved.push(ResolvedPlugin {
            name: name.clone(),
            crate_name,
            dependency,
        });
    }
    Ok(resolved)
}

/// A plugin name is a lowercase, digit, or hyphen segment that starts and
/// ends with an alphanumeric character, the same shape Cargo accepts.
pub(crate) fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('-')
        && !name.ends_with('-')
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// The dependency path for a plugin crate directory.
///
/// The generated crate lives in `<project>/generated`, so a plugin path
/// relative to the project directory becomes one directory up plus that
/// path. An absolute path stays as written. A directory without a
/// `Cargo.toml` is not a plugin crate, so `resolve` reports it.
fn path_dependency(project_dir: &Path, path: &str) -> Option<String> {
    let plugin_dir = if Path::new(path).is_absolute() {
        Path::new(path).to_path_buf()
    } else {
        project_dir.join(path)
    };
    if !plugin_dir.join("Cargo.toml").is_file() {
        return None;
    }
    if Path::new(path).is_absolute() {
        return Some(path.to_string());
    }
    Some(format!("../{path}"))
}

fn unresolvable(detail: String) -> Diagnostic {
    Diagnostic::blocker(
        CODE_PLUGIN,
        format!("cannot compose the plugin: {detail}"),
        format!(
            "fix the `[plugins]` entry in `rivet.toml`: {detail}. Set `path` to the plugin crate directory relative to the project, or set `version` for a published `rivet-plugin-<name>` crate, then rerun `rivet build`"
        ),
    )
    .located("rivet.toml", 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::ScratchDir;
    use std::fs;

    fn config_with(name: &str, path: Option<&str>, version: Option<&str>) -> RivetConfig {
        let mut config = RivetConfig::default();
        config.plugins.insert(
            name.to_string(),
            crate::config::PluginConfig {
                crate_name: None,
                path: path.map(str::to_string),
                version: version.map(str::to_string),
            },
        );
        config
    }

    #[test]
    fn a_path_plugin_resolves_to_a_generated_relative_dependency() {
        let dir = ScratchDir::new("plugin-path");
        let plugin_dir = dir.join("plugins").join("auth-token");
        fs::create_dir_all(plugin_dir.join("src")).expect("create plugin dir");
        fs::write(plugin_dir.join("Cargo.toml"), "[package]\n").expect("write manifest");

        let resolved = resolve(
            &config_with("auth-token", Some("plugins/auth-token"), None),
            &dir,
        )
        .expect("resolve");
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].name, "auth-token");
        assert_eq!(resolved[0].crate_name, "rivet-plugin-auth-token");
        assert_eq!(resolved[0].crate_ident(), "rivet_plugin_auth_token");
        assert_eq!(
            resolved[0].dependency,
            "rivet-plugin-auth-token = { path = \"../plugins/auth-token\" }"
        );
    }

    #[test]
    fn a_version_plugin_resolves_to_a_registry_dependency() {
        let dir = ScratchDir::new("plugin-version");
        let resolved =
            resolve(&config_with("audit-log", None, Some("0.3")), &dir).expect("resolve");
        assert_eq!(resolved[0].dependency, "rivet-plugin-audit-log = \"0.3\"");
    }

    #[test]
    fn a_plugin_path_without_a_manifest_is_unresolvable() {
        let dir = ScratchDir::new("plugin-missing");
        fs::create_dir_all(dir.join("plugins/nothing")).expect("create empty dir");
        let diagnostic = resolve(&config_with("nothing", Some("plugins/nothing"), None), &dir)
            .expect_err("a plugin directory without a manifest cannot resolve");
        assert_eq!(diagnostic.error_code, "E3016");
        assert!(diagnostic.suggested_fix.contains("rivet.toml"));
    }

    #[test]
    fn a_plugin_without_a_path_or_version_is_unresolvable() {
        let dir = ScratchDir::new("plugin-bare");
        let diagnostic = resolve(&config_with("bare", None, None), &dir)
            .expect_err("a plugin needs a path or a version");
        assert_eq!(diagnostic.error_code, "E3016");
        assert!(diagnostic.message.contains("neither `path` nor `version`"));
    }

    #[test]
    fn an_invalid_plugin_name_is_unresolvable() {
        let dir = ScratchDir::new("plugin-name");
        let diagnostic = resolve(&config_with("Auth_Token", None, Some("0.1")), &dir)
            .expect_err("an invalid plugin name cannot resolve");
        assert_eq!(diagnostic.error_code, "E3016");
        assert!(diagnostic.message.contains("not a crate-name segment"));
    }

    #[test]
    fn no_plugins_resolves_to_nothing() {
        let dir = ScratchDir::new("plugin-none");
        let resolved = resolve(&RivetConfig::default(), &dir).expect("resolve");
        assert!(resolved.is_empty());
    }
}
