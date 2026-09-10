//! `rivet add`: record project dependencies in `rivet.toml`.
//!
//! The command edits the project configuration in place and keeps every
//! other line — comments included — byte for byte. It validates the edited
//! document by re-parsing it as a [`RivetConfig`] before it writes, so a
//! broken configuration never reaches disk.

use crate::config::RivetConfig;
use crate::diagnostic::Diagnostic;
use crate::plugin;
use std::path::{Path, PathBuf};
use toml_edit::{DocumentMut, Item, Table, value};

/// Error code for a plugin declaration that cannot resolve.
const CODE_PLUGIN: &str = "E3016";
/// Error code for a `rivet.toml` that cannot be read, parsed, or written.
const CODE_CONFIG: &str = "E3017";

/// The plugin entry to write: the table key plus the values to set.
struct EntryUpdate<'a> {
    name: &'a str,
    path: Option<&'a str>,
    crate_name: Option<&'a str>,
    version: Option<&'a str>,
}

impl<'a> EntryUpdate<'a> {
    /// The table keys to write, in the order the file spells them.
    fn keys(&self) -> Vec<(&'static str, &'a str)> {
        let mut keys = Vec::new();
        if let Some(crate_name) = self.crate_name {
            keys.push(("crate", crate_name));
        }
        if let Some(path) = self.path {
            keys.push(("path", path));
        }
        if let Some(version) = self.version {
            keys.push(("version", version));
        }
        keys
    }
}

/// Record or update a `[plugins.<name>]` entry, then keep the rest of the
/// file as it was. The command is idempotent: the same call twice writes the
/// same file.
pub fn run_add_plugin(
    app_file: &Path,
    name: &str,
    path: Option<&str>,
    crate_name: Option<&str>,
    version: Option<&str>,
) -> Result<(), Vec<Diagnostic>> {
    let update = EntryUpdate {
        name,
        path,
        crate_name,
        version,
    };
    let project_dir = project_dir_for(app_file);
    if !plugin::valid_name(name) {
        return Err(vec![plugin_error(format!(
            "plugin name `{name}` is not a crate-name segment"
        ))]);
    }
    if let Some(path) = path
        && !project_dir.join(path).join("Cargo.toml").is_file()
    {
        return Err(vec![plugin_error(format!(
            "plugin `{name}` points at `{path}`, which has no `Cargo.toml`"
        ))]);
    }

    let config_path = project_dir.join("rivet.toml");
    let edited = match read_config(&config_path)? {
        Some(raw) => edit_existing(&raw, &config_path, &update)?,
        // A project with no configuration gets one written entry and no
        // empty parent table.
        None => new_document(&update),
    };
    let config: RivetConfig = toml::from_str(&edited)
        .map_err(|err| vec![config_error(&config_path, format!("{err}"))])?;
    // The same check `rivet build` runs, so `add` cannot write a plugin the
    // build would reject.
    let resolved = plugin::resolve(&config, &project_dir).map_err(|diagnostic| vec![diagnostic])?;
    let resolved = resolved
        .iter()
        .find(|resolved| resolved.name == name)
        .ok_or_else(|| vec![plugin_error(format!("plugin `{name}` did not record"))])?;

    std::fs::write(&config_path, &edited).map_err(|err| {
        vec![config_error(
            &config_path,
            format!("could not write the file: {err}"),
        )]
    })?;
    println!(
        "Recorded plugin {name:?} as {} in {}",
        resolved.crate_name,
        config_path.display()
    );
    Ok(())
}

/// The project directory that owns `rivet.toml`, resolved like every other
/// command resolves its store.
fn project_dir_for(app_file: &Path) -> PathBuf {
    app_file
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Read `rivet.toml`, or report `None` when the project has none yet.
fn read_config(config_path: &Path) -> Result<Option<String>, Vec<Diagnostic>> {
    match std::fs::read_to_string(config_path) {
        Ok(raw) => Ok(Some(raw)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(vec![config_error(
            config_path,
            format!("could not read the file: {err}"),
        )]),
    }
}

/// Insert or update `[plugins.<name>]` in an existing document.
///
/// A parent table this command creates is marked implicit, so the file gains
/// the entry and no bare `[plugins]` header. Every other line, comments
/// included, survives the parse-and-render round trip.
fn edit_existing(
    raw: &str,
    config_path: &Path,
    update: &EntryUpdate<'_>,
) -> Result<String, Vec<Diagnostic>> {
    let mut document = raw
        .parse::<DocumentMut>()
        .map_err(|err| vec![config_error(config_path, format!("{err}"))])?;
    {
        let root = document.as_table_mut();
        if root.get("plugins").is_some_and(|item| !item.is_table()) {
            return Err(vec![config_error(
                config_path,
                "the `plugins` key is not a table".to_string(),
            )]);
        }
        let created_table = root.get("plugins").is_none();
        let plugins = root
            .entry("plugins")
            .or_insert(Item::Table(Table::new()))
            .as_table_mut()
            .ok_or_else(|| {
                vec![config_error(
                    config_path,
                    "the `plugins` key is not a table".to_string(),
                )]
            })?;
        if created_table {
            plugins.set_implicit(true);
        }
        let entry = plugins
            .entry(update.name)
            .or_insert(Item::Table(Table::new()))
            .as_table_mut()
            .ok_or_else(|| {
                vec![config_error(
                    config_path,
                    format!("`plugins.{}` is not a table", update.name),
                )]
            })?;
        for (key, text) in update.keys() {
            let _ = entry.insert(key, value(text));
        }
        if !entry.contains_key("path") && !entry.contains_key("version") {
            return Err(vec![plugin_error(format!(
                "plugin `{}` sets neither `path` nor `version`",
                update.name
            ))]);
        }
    }
    Ok(document.to_string())
}

/// The document to write for a project that has no configuration yet.
///
/// The shape is fixed and small: a header comment and the one plugin table.
fn new_document(update: &EntryUpdate<'_>) -> String {
    let mut out = String::from("# Rivet project configuration.\n\n");
    out.push_str(&format!("[plugins.{}]\n", update.name));
    for (key, text) in update.keys() {
        out.push_str(&format!("{key} = {text:?}\n"));
    }
    out
}

fn plugin_error(detail: String) -> Diagnostic {
    Diagnostic::blocker(
        CODE_PLUGIN,
        format!("cannot record the plugin: {detail}"),
        format!(
            "pass the plugin source: {detail}. Use `--path <dir>` for a plugin crate in this project, or `--version <v>` for a published `rivet-plugin-<name>` crate"
        ),
    )
    .located("rivet.toml", 1)
}

fn config_error(config_path: &Path, detail: String) -> Diagnostic {
    Diagnostic::blocker(
        CODE_CONFIG,
        format!("cannot edit {}: {detail}", config_path.display()),
        format!(
            "fix {} so `rivet add` can update it: {detail}",
            config_path.display()
        ),
    )
    .located(config_path.display().to_string(), 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::ScratchDir;
    use std::fs;

    /// A project with a plugin crate and a `rivet.toml` that carries a
    /// comment worth preserving.
    fn project(name: &str) -> ScratchDir {
        let dir = ScratchDir::new(name);
        fs::create_dir_all(dir.join("plugins/auth-token/src")).expect("create plugin dir");
        fs::write(dir.join("plugins/auth-token/Cargo.toml"), "[package]\n")
            .expect("write plugin manifest");
        fs::write(
            dir.join("rivet.toml"),
            "# Kept by the test.\n[project]\nname = \"basic\"\n",
        )
        .expect("write rivet.toml");
        dir
    }

    #[test]
    fn add_records_a_path_plugin_and_keeps_the_rest_of_the_file() {
        let dir = project("add-plugin");
        run_add_plugin(
            &dir.join("app.py"),
            "auth-token",
            Some("plugins/auth-token"),
            None,
            None,
        )
        .expect("add plugin");

        let raw = fs::read_to_string(dir.join("rivet.toml")).expect("read rivet.toml");
        assert!(raw.contains("# Kept by the test."), "{raw}");
        assert!(raw.contains("[project]"), "{raw}");
        assert!(raw.contains("[plugins.auth-token]"), "{raw}");
        assert!(
            !raw.contains("[plugins]\n"),
            "adding to a config without a plugins table adds no bare header:\n{raw}"
        );
        assert!(raw.contains("path = \"plugins/auth-token\""), "{raw}");
        let config: RivetConfig = toml::from_str(&raw).expect("the file still parses");
        assert_eq!(
            config
                .plugins
                .get("auth-token")
                .and_then(|plugin| plugin.path.as_deref()),
            Some("plugins/auth-token")
        );
    }

    #[test]
    fn add_is_idempotent() {
        let dir = project("add-idempotent");
        let app = dir.join("app.py");
        run_add_plugin(&app, "auth-token", Some("plugins/auth-token"), None, None)
            .expect("first add");
        let first = fs::read_to_string(dir.join("rivet.toml")).expect("read rivet.toml");
        run_add_plugin(&app, "auth-token", Some("plugins/auth-token"), None, None)
            .expect("second add");
        let second = fs::read_to_string(dir.join("rivet.toml")).expect("read rivet.toml");
        assert_eq!(first, second);
    }

    #[test]
    fn add_updates_only_the_named_plugin() {
        let dir = project("add-two-plugins");
        let app = dir.join("app.py");
        run_add_plugin(&app, "auth-token", Some("plugins/auth-token"), None, None)
            .expect("add auth-token");
        run_add_plugin(&app, "audit-log", None, None, Some("0.3")).expect("add audit-log");

        let raw = fs::read_to_string(dir.join("rivet.toml")).expect("read rivet.toml");
        let config: RivetConfig = toml::from_str(&raw).expect("parse");
        assert_eq!(config.plugins.len(), 2);
        assert_eq!(
            config
                .plugins
                .get("audit-log")
                .and_then(|plugin| plugin.version.as_deref()),
            Some("0.3")
        );
    }

    #[test]
    fn add_creates_the_file_when_the_project_has_none() {
        let dir = ScratchDir::new("add-no-config");
        fs::create_dir_all(dir.join("plugins/auth-token/src")).expect("create plugin dir");
        fs::write(dir.join("plugins/auth-token/Cargo.toml"), "[package]\n")
            .expect("write plugin manifest");
        run_add_plugin(
            &dir.join("app.py"),
            "auth-token",
            Some("plugins/auth-token"),
            None,
            None,
        )
        .expect("add plugin");
        let raw = fs::read_to_string(dir.join("rivet.toml")).expect("read rivet.toml");
        assert!(raw.contains("[plugins.auth-token]"), "{raw}");
        assert!(
            !raw.contains("[plugins]\n"),
            "a fresh file gains no empty `[plugins]` header:\n{raw}"
        );
        assert!(raw.contains("path = \"plugins/auth-token\""), "{raw}");
    }

    #[test]
    fn add_rejects_a_plugin_without_a_source() {
        let dir = project("add-bare");
        let diagnostics = run_add_plugin(&dir.join("app.py"), "auth-token", None, None, None)
            .expect_err("a plugin needs a source");
        assert_eq!(diagnostics[0].error_code, "E3016");
        assert!(!diagnostics[0].suggested_fix.is_empty());
        assert!(
            !fs::read_to_string(dir.join("rivet.toml"))
                .expect("read")
                .contains("[plugins.auth-token]"),
            "a rejected plugin must not be written"
        );
    }

    #[test]
    fn add_rejects_a_plugin_path_without_a_manifest() {
        let dir = project("add-missing-crate");
        let diagnostics = run_add_plugin(
            &dir.join("app.py"),
            "auth-token",
            Some("plugins/gone"),
            None,
            None,
        )
        .expect_err("a plugin directory needs a manifest");
        assert_eq!(diagnostics[0].error_code, "E3016");
    }

    #[test]
    fn add_reports_a_malformed_config_without_touching_it() {
        let dir = project("add-malformed");
        let raw = "this is not toml\n";
        fs::write(dir.join("rivet.toml"), raw).expect("write rivet.toml");
        let diagnostics = run_add_plugin(
            &dir.join("app.py"),
            "auth-token",
            Some("plugins/auth-token"),
            None,
            None,
        )
        .expect_err("a malformed config cannot be edited");
        assert_eq!(diagnostics[0].error_code, "E3017");
        assert_eq!(
            fs::read_to_string(dir.join("rivet.toml")).expect("read"),
            raw,
            "the failed edit must leave the file unchanged"
        );
    }
}
