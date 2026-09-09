//! `rivet history`: list recorded commands with exit status and timing.
//!
//! Every invocation is recorded in the project store before it runs (see
//! [`crate::store`]); this command reads those rows back, newest first.
//! The store lives next to the app module (`.rivet/rivet.db`), so `history`
//! takes the same `app` argument as `build` and `audit`.

use crate::diagnostic::Diagnostic;
use crate::store;
use std::path::Path;

/// Print up to 50 recorded commands, newest first.
///
/// Each line shows the row id, command label, exit status, and duration.
/// A store that has never recorded a command prints a short notice and
/// succeeds, so the command is safe to run in a fresh project.
pub fn run_history(app_file: &Path) -> Result<(), Vec<Diagnostic>> {
    let project_dir = store::project_dir_for(app_file);
    let conn = store::open(&project_dir)?;

    let records = store::list_commands(&conn, 50)?;
    if records.is_empty() {
        println!("No commands recorded yet in {}", project_dir.display());
        return Ok(());
    }

    println!(
        "{:<4} {:<24} {:>6} {:>12}",
        "id", "command", "exit", "duration"
    );
    for record in records {
        let status = record
            .exit_status
            .map_or_else(|| "?".into(), |s| s.to_string());
        let duration = record
            .duration_ms
            .map_or_else(|| "-".into(), |ms| format!("{ms} ms"));
        println!(
            "{:<4} {:<24} {:>6} {:>12}",
            record.id, record.command, status, duration
        );
    }
    Ok(())
}
