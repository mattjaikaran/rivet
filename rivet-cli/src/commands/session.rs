//! `rivet session`: save, list, and resume compact markdown context.
//!
//! A session captures what a human or agent was looking at: the parsed
//! module summary, the `[gauntlet]` config that ran, and the diagnostics it
//! produced. `save` renders that context to compact markdown and stores it
//! in the project store; `resume` prints it back verbatim so work can
//! continue where it stopped; `list` shows the saved session names.
use crate::config::RivetConfig;
use crate::diagnostic::Diagnostic;
use crate::gauntlet;
use crate::parser::python::parse_python_file;
use crate::store;
use std::path::Path;

/// Save the current module context as a named session.
///
/// The context is rendered from a fresh parse of the app module so the
/// stored markdown always reflects the file on disk, then stored under
/// `name`. Saving an existing name replaces its content.
pub fn run_session_save(app_file: &Path, name: &str) -> Result<(), Vec<Diagnostic>> {
    let project_dir = store::project_dir_for(app_file);
    let conn = store::open(&project_dir)?;

    let markdown = render_context(app_file).map_err(|diagnostic| vec![diagnostic])?;
    store::save_session(&conn, name, &markdown)?;
    println!("Saved session {name:?} in {}", project_dir.display());
    Ok(())
}

/// Print a saved session context so work can continue.
pub fn run_session_resume(app_file: &Path, name: &str) -> Result<(), Vec<Diagnostic>> {
    let project_dir = store::project_dir_for(app_file);
    let conn = store::open(&project_dir)?;

    match store::load_session(&conn, name)? {
        Some(record) => {
            print!("{}", record.content);
            Ok(())
        }
        None => Err(vec![Diagnostic::blocker(
            "E3010",
            format!("no session named {name:?} in {}", project_dir.display()),
        )]),
    }
}

/// List the saved session names.
pub fn run_session_list(app_file: &Path) -> Result<(), Vec<Diagnostic>> {
    let project_dir = store::project_dir_for(app_file);
    let conn = store::open(&project_dir)?;

    let sessions = store::list_sessions(&conn)?;
    if sessions.is_empty() {
        println!("No sessions saved yet in {}", project_dir.display());
        return Ok(());
    }
    for session in sessions {
        println!("{:<20} saved at epoch {}", session.name, session.created_at);
    }
    Ok(())
}

/// Render the current command context as compact markdown.
///
/// The heading records the app path; the body summarizes the parsed module
/// (routes and DTOs), the `[gauntlet]` config in effect, and the diagnostics
/// the module produced. Human and agent resume from this file.
pub(crate) fn render_context(app_file: &Path) -> Result<String, Diagnostic> {
    let project_dir = store::project_dir_for(app_file);
    let config =
        RivetConfig::load(&project_dir).map_err(|message| Diagnostic::blocker("E1008", message))?;
    let module = parse_python_file(app_file)?;
    let findings = gauntlet::run_gauntlet(&module, &config.gauntlet);

    let mut out = String::new();
    out.push_str(&format!("# Rivet session for {}\n\n", app_file.display()));

    let blueprint = &module.blueprint;
    out.push_str(&format!("## Blueprint: {}\n\n", blueprint.name));
    for route in &blueprint.routes {
        let method = route.method.as_str();
        out.push_str(&format!("- `{method} {}`", route.path));
        if !route.stories.is_empty() {
            out.push_str(&format!(" (stories: {})", route.stories.join(", ")));
        }
        out.push('\n');
    }
    if blueprint.routes.is_empty() {
        out.push_str("- no routes\n");
    }

    if !blueprint.structs.is_empty() {
        out.push_str("## DTOs\n\n");
        for struct_def in &blueprint.structs {
            out.push_str(&format!("- `{}`\n", struct_def.name));
        }
        out.push('\n');
    }

    out.push_str("## Gauntlet config\n\n");
    let gauntlet = &config.gauntlet;
    out.push_str(&format!("- max_complexity: {}\n", gauntlet.max_complexity));
    out.push_str(&format!(
        "- stories_required: {}\n",
        gauntlet.stories_required
    ));
    out.push_str(&format!(
        "- strict_type_checking: {}\n",
        gauntlet.strict_type_checking
    ));
    out.push_str(&format!(
        "- duplicate_code: {:?}\n",
        gauntlet.duplicate_code
    ));
    out.push_str(&format!("- dead_code: {:?}\n", gauntlet.dead_code));
    out.push('\n');

    out.push_str("## Diagnostics\n\n");
    if findings.is_empty() {
        out.push_str("- none\n");
    } else {
        for finding in &findings {
            let severity = match finding.severity {
                crate::diagnostic::Severity::Warning => "warning",
                crate::diagnostic::Severity::Blocker => "blocker",
            };
            out.push_str(&format!(
                "- `{severity}` {}: {}\n",
                finding.error_code, finding.message
            ));
        }
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn render_context_round_trips_module_summary() {
        // Write a tiny app into a temp dir with a rivet.toml so the config
        // resolves, parse it, and confirm the markdown names the route.
        let dir = std::env::temp_dir().join(format!("rivet-session-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let app = dir.join("app.py");
        std::fs::write(
            &app,
            "from rivet import api\n\n@api.get(\"/ping\", stories=[\"US-001\"])\ndef ping() -> dict:\n    return {\"status\": \"pong\"}\n",
        )
        .unwrap();
        let markdown = render_context(&app).expect("context renders");
        assert!(markdown.contains("GET /ping"), "{markdown}");
        assert!(markdown.contains("US-001"), "{markdown}");
        assert!(markdown.contains("## Gauntlet config"), "{markdown}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn project_dir_for_resolves_store_location() {
        let base = PathBuf::from("fixtures/x");
        assert_eq!(store::project_dir_for(&base.join("app.py")), base);
        assert_eq!(
            store::project_dir_for(&PathBuf::from("app.py")),
            PathBuf::from(".")
        );
    }
}
