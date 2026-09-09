//! `rivet history`: list recorded commands with exit status and timing.
//!
//! Every invocation is recorded in the project store before it runs (see
//! [`crate::store`]); this command reads those rows back, newest first.
//! The store lives next to the app module (`.rivet/rivet.db`), so `history`
//! takes the same `app` argument as `build` and `audit`.

use crate::diagnostic::Diagnostic;
use crate::store;
use std::path::Path;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

/// Render an epoch-seconds timestamp as an RFC 3339 UTC string.
fn render_time(epoch_secs: i64) -> String {
    match OffsetDateTime::from_unix_timestamp(epoch_secs) {
        Ok(dt) => dt
            .format(&Rfc3339)
            .unwrap_or_else(|_| epoch_secs.to_string()),
        Err(_) => epoch_secs.to_string(),
    }
}

/// Print up to 50 recorded commands, newest first.
///
/// Each line shows the row id, command label, invocation time, exit status,
/// and duration. A store that has never recorded a command prints a short
/// notice and succeeds, so the command is safe to run in a fresh project.
pub fn run_history(app_file: &Path) -> Result<(), Vec<Diagnostic>> {
    let project_dir = store::project_dir_for(app_file);
    let conn = store::open(&project_dir)?;

    let records = store::list_commands(&conn, 50)?;
    if records.is_empty() {
        println!("No commands recorded yet in {}", project_dir.display());
        return Ok(());
    }

    println!(
        "{:<4} {:<24} {:<25} {:>6} {:>10}",
        "id", "command", "invoked at", "exit", "duration"
    );
    for record in records {
        let status = record
            .exit_status
            .map_or_else(|| "?".into(), |s| s.to_string());
        let duration = record
            .duration_ms
            .map_or_else(|| "-".into(), |ms| format!("{ms} ms"));
        let when = render_time(record.invoked_at);
        println!(
            "{:<4} {:<24} {:<25} {:>6} {:>10}",
            record.id, record.command, when, status, duration
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_time_formats_epoch_as_utc() {
        let rendered = render_time(1_788_912_000);
        assert!(rendered.starts_with("2026-09-09T00:00:00"), "{rendered}");
    }

    #[test]
    fn render_time_falls_back_on_out_of_range() {
        assert_eq!(render_time(i64::MAX), i64::MAX.to_string());
    }
}
