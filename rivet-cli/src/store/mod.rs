//! Local context store for the Rivet CLI (phase 2, pillar 04).
//!
//! The store persists what the CLI does so humans and agents can resume it:
//! every command invocation, saved session context, and per-commit AST
//! fingerprints. It lives on disk at `<project_dir>/.rivet/rivet.db`
//! (SQLite), one store per project. SQLite access stays in this module and
//! never in `rivet-core`, keeping the core WASM-friendly.
//!
//! Commands are recorded before they run: [`start_command`] inserts a row
//! with a null status, then [`finish_command`] fills in the exit status and
//! duration when the command returns. A row with a null status is an
//! invocation that never finished (for example, the process was killed).
//!
//! Schema migration is idempotent: opening a store creates the file if it
//! is missing and applies pending migrations, so a fresh or stale store
//! converges to the current schema.
//!
//! Error codes owned by the context engine: `E3000`-`E3010`.

pub mod vector;
use crate::diagnostic::Diagnostic;
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The current schema version. Bump when adding a migration.
const SCHEMA_VERSION: i64 = 1;

/// One recorded CLI invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandRecord {
    /// Row id, ascending in invocation order.
    pub id: i64,
    /// Command name and its arguments, exactly as invoked.
    pub command: String,
    /// Unix timestamp of the invocation, seconds since the epoch.
    pub invoked_at: i64,
    /// Process exit status. `None` means the invocation never finished.
    pub exit_status: Option<i64>,
    /// Wall-clock duration of the invocation, in milliseconds.
    pub duration_ms: Option<i64>,
}

/// A saved session context blob.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRecord {
    /// Session name, unique per project.
    pub name: String,
    /// Unix timestamp of when the session was saved.
    pub created_at: i64,
    /// The compact markdown context the session holds.
    pub content: String,
}

/// Open the project store, creating `.rivet/rivet.db` and migrating it.
pub fn open(project_dir: &Path) -> Result<Connection, Diagnostic> {
    let dir = project_dir.join(".rivet");
    std::fs::create_dir_all(&dir)
        .map_err(|err| store_error("E3000", format!("cannot create store directory: {err}"), "free disk space or fix write permissions on the project directory so the CLI can create `.rivet/rivet.db`, then rerun the command"))?;
    let path = dir.join("rivet.db");
    let conn = Connection::open(&path)
        .map_err(|err| store_error("E3000", format!("cannot open {}: {err}", path.display()), "free disk space or fix write permissions on the project directory so the CLI can create `.rivet/rivet.db`, then rerun the command"))?;
    migrate(&conn)?;
    Ok(conn)
}

/// Apply pending schema migrations. Idempotent: safe to run on every open.
fn migrate(conn: &Connection) -> Result<(), Diagnostic> {
    let current: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(|err| store_error("E3001", format!("cannot read schema version: {err}"), "delete `.rivet/rivet.db` so the CLI recreates it, then rerun the command; a store written by a newer `rivet` needs the matching binary"))?;
    if current >= SCHEMA_VERSION {
        return Ok(());
    }
    conn.execute_batch(
        "BEGIN;
         CREATE TABLE IF NOT EXISTS commands (
             id INTEGER PRIMARY KEY AUTOINCREMENT,
             command TEXT NOT NULL,
             invoked_at INTEGER NOT NULL,
             exit_status INTEGER,
             duration_ms INTEGER
         );
         CREATE TABLE IF NOT EXISTS sessions (
             name TEXT PRIMARY KEY,
             created_at INTEGER NOT NULL,
             content TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS fingerprints (
             commit_hash TEXT NOT NULL,
             app_path TEXT NOT NULL,
             digest TEXT NOT NULL,
             PRIMARY KEY (commit_hash, app_path)
         );
         PRAGMA user_version = 1;
         COMMIT;",
    )
    .map_err(|err| store_error("E3001", format!("schema migration failed: {err}"), "delete `.rivet/rivet.db` so the CLI recreates it, then rerun the command; a store written by a newer `rivet` needs the matching binary"))?;
    Ok(())
}

/// Insert a command record before it runs and return its row id.
pub fn start_command(conn: &Connection, command: &str) -> Result<i64, Diagnostic> {
    conn.execute(
        "INSERT INTO commands (command, invoked_at) VALUES (?1, ?2)",
        rusqlite::params![command, now_epoch_secs()],
    )
    .map_err(|err| store_error("E3002", format!("cannot record command: {err}"), "the command log is corrupt or the disk is full; delete `.rivet/rivet.db` to reset it (or free disk space and fix `.rivet` permissions), then rerun the command"))?;
    Ok(conn.last_insert_rowid())
}

/// Fill in the exit status and duration of a started command.
pub fn finish_command(
    conn: &Connection,
    id: i64,
    exit_status: i64,
    duration: Duration,
) -> Result<(), Diagnostic> {
    let duration_ms = duration.as_millis().min(i64::MAX as u128) as i64;
    conn.execute(
        "UPDATE commands SET exit_status = ?1, duration_ms = ?2 WHERE id = ?3",
        rusqlite::params![exit_status, duration_ms, id],
    )
    .map_err(|err| store_error("E3002", format!("cannot finish command: {err}"), "the command log is corrupt or the disk is full; delete `.rivet/rivet.db` to reset it (or free disk space and fix `.rivet` permissions), then rerun the command"))?;
    Ok(())
}

/// List finished commands newest-first, limited to `limit` rows.
pub fn list_commands(conn: &Connection, limit: usize) -> Result<Vec<CommandRecord>, Diagnostic> {
    let mut stmt = conn
        .prepare(
            "SELECT id, command, invoked_at, exit_status, duration_ms
             FROM commands
             WHERE exit_status IS NOT NULL
             ORDER BY id DESC LIMIT ?1",
        )
        .map_err(|err| store_error("E3002", format!("cannot query commands: {err}"), "the command log is corrupt or the disk is full; delete `.rivet/rivet.db` to reset it (or free disk space and fix `.rivet` permissions), then rerun the command"))?;
    let rows = stmt
        .query_map(rusqlite::params![limit as i64], |row| {
            Ok(CommandRecord {
                id: row.get(0)?,
                command: row.get(1)?,
                invoked_at: row.get(2)?,
                exit_status: row.get(3)?,
                duration_ms: row.get(4)?,
            })
        })
        .map_err(|err| store_error("E3002", format!("cannot query commands: {err}"), "the command log is corrupt or the disk is full; delete `.rivet/rivet.db` to reset it (or free disk space and fix `.rivet` permissions), then rerun the command"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|err| store_error("E3002", format!("cannot read commands: {err}"), "the command log is corrupt or the disk is full; delete `.rivet/rivet.db` to reset it (or free disk space and fix `.rivet` permissions), then rerun the command"))
}

/// Save a session context blob, replacing an existing session of the same
/// name.
pub fn save_session(conn: &Connection, name: &str, content: &str) -> Result<(), Diagnostic> {
    conn.execute(
        "INSERT INTO sessions (name, created_at, content) VALUES (?1, ?2, ?3)
         ON CONFLICT(name) DO UPDATE SET created_at = ?2, content = ?3",
        rusqlite::params![name, now_epoch_secs(), content],
    )
    .map_err(|err| store_error("E3003", format!("cannot save session: {err}"), "the session table is corrupt or the disk is full; delete `.rivet/rivet.db` to reset it (or free disk space and fix `.rivet` permissions), then rerun the command"))?;
    Ok(())
}

/// Load a saved session by name.
pub fn load_session(conn: &Connection, name: &str) -> Result<Option<SessionRecord>, Diagnostic> {
    let mut stmt = conn
        .prepare("SELECT name, created_at, content FROM sessions WHERE name = ?1")
        .map_err(|err| store_error("E3003", format!("cannot query session: {err}"), "the session table is corrupt or the disk is full; delete `.rivet/rivet.db` to reset it (or free disk space and fix `.rivet` permissions), then rerun the command"))?;
    let mut rows = stmt
        .query_map(rusqlite::params![name], |row| {
            Ok(SessionRecord {
                name: row.get(0)?,
                created_at: row.get(1)?,
                content: row.get(2)?,
            })
        })
        .map_err(|err| store_error("E3003", format!("cannot query session: {err}"), "the session table is corrupt or the disk is full; delete `.rivet/rivet.db` to reset it (or free disk space and fix `.rivet` permissions), then rerun the command"))?;
    rows.next()
        .transpose()
        .map_err(|err| store_error("E3003", format!("cannot read session: {err}"), "the session table is corrupt or the disk is full; delete `.rivet/rivet.db` to reset it (or free disk space and fix `.rivet` permissions), then rerun the command"))
}

/// List saved session names and timestamps, oldest first.
pub fn list_sessions(conn: &Connection) -> Result<Vec<SessionRecord>, Diagnostic> {
    let mut stmt = conn
        .prepare("SELECT name, created_at, content FROM sessions ORDER BY created_at")
        .map_err(|err| store_error("E3003", format!("cannot query sessions: {err}"), "the session table is corrupt or the disk is full; delete `.rivet/rivet.db` to reset it (or free disk space and fix `.rivet` permissions), then rerun the command"))?;
    let rows = stmt
        .query_map([], |row| {
            Ok(SessionRecord {
                name: row.get(0)?,
                created_at: row.get(1)?,
                content: row.get(2)?,
            })
        })
        .map_err(|err| store_error("E3003", format!("cannot query sessions: {err}"), "the session table is corrupt or the disk is full; delete `.rivet/rivet.db` to reset it (or free disk space and fix `.rivet` permissions), then rerun the command"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|err| store_error("E3003", format!("cannot read sessions: {err}"), "the session table is corrupt or the disk is full; delete `.rivet/rivet.db` to reset it (or free disk space and fix `.rivet` permissions), then rerun the command"))
}

/// Store an AST fingerprint for one commit and app path.
pub fn save_fingerprint(
    conn: &Connection,
    commit: &str,
    app_path: &str,
    digest: &str,
) -> Result<(), Diagnostic> {
    conn.execute(
        "INSERT INTO fingerprints (commit_hash, app_path, digest) VALUES (?1, ?2, ?3)
         ON CONFLICT(commit_hash, app_path) DO UPDATE SET digest = ?3",
        rusqlite::params![commit, app_path, digest],
    )
    .map_err(|err| store_error("E3004", format!("cannot save fingerprint: {err}"), "the fingerprint table is corrupt or the disk is full; delete `.rivet/rivet.db` to reset it (or free disk space and fix `.rivet` permissions), then rerun the command"))?;
    Ok(())
}

/// Build a diagnostic with a context-engine error code.
fn store_error(code: &str, message: String, fix: &str) -> Diagnostic {
    Diagnostic::blocker(code, message, fix)
}

/// Seconds since the Unix epoch, as an i64.
pub(crate) fn now_epoch_secs() -> i64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_secs() as i64,
        Err(_) => 0,
    }
}

/// Resolve the project directory that owns a store for an app path.
///
/// The store lives next to the app module: the directory holding the app
/// file, or the current directory when the app is a bare filename.
pub fn project_dir_for(app_file: &Path) -> PathBuf {
    app_file
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Open an isolated store under a fresh temp dir; returns the dir path
    /// so the test can clean it up. The name mixes the process id and a
    /// per-test counter so parallel tests never share a directory.
    fn temp_store() -> (PathBuf, Connection) {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "rivet-store-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let conn = open(&dir).expect("store opens on an empty dir");
        (dir, conn)
    }

    #[test]
    fn migration_is_idempotent_and_creates_tables() {
        let (dir, conn) = temp_store();
        migrate(&conn).expect("second migrate is a no-op");
        let count: i64 = conn
            .query_row("SELECT count(*) FROM commands", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn recorded_commands_list_newest_first() {
        let (dir, conn) = temp_store();
        let first = start_command(&conn, "build app.py").unwrap();
        let second = start_command(&conn, "audit app.py").unwrap();
        finish_command(&conn, first, 0, Duration::from_millis(10)).unwrap();
        finish_command(&conn, second, 1, Duration::from_millis(20)).unwrap();
        let rows = list_commands(&conn, 10).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].command, "audit app.py");
        assert_eq!(rows[0].exit_status, Some(1));
        assert_eq!(rows[0].duration_ms, Some(20));
        assert_eq!(rows[1].command, "build app.py");
        assert_eq!(rows[1].exit_status, Some(0));
        assert!(rows[0].invoked_at >= rows[1].invoked_at);
        let limited = list_commands(&conn, 1).unwrap();
        assert_eq!(limited.len(), 1);
        assert_eq!(limited[0].command, "audit app.py");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unfinished_commands_are_hidden_from_history() {
        let (dir, conn) = temp_store();
        let id = start_command(&conn, "kill me").unwrap();
        let rows = list_commands(&conn, 10).unwrap();
        assert!(rows.is_empty());
        finish_command(&conn, id, 137, Duration::from_millis(5)).unwrap();
        let rows = list_commands(&conn, 10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].exit_status, Some(137));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sessions_round_trip_and_overwrite() {
        let (dir, conn) = temp_store();
        save_session(&conn, "debug", "# context").unwrap();
        save_session(&conn, "debug", "# context v2").unwrap();
        save_session(&conn, "other", "# other").unwrap();
        let loaded = load_session(&conn, "debug")
            .unwrap()
            .expect("session exists");
        assert_eq!(loaded.name, "debug");
        assert_eq!(loaded.content, "# context v2");
        assert!(load_session(&conn, "missing").unwrap().is_none());
        let all = list_sessions(&conn).unwrap();
        assert_eq!(all.len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn project_dir_is_app_parent_or_current_dir() {
        assert_eq!(
            project_dir_for(Path::new("examples/basic/app.py")),
            Path::new("examples/basic")
        );
        assert_eq!(project_dir_for(Path::new("app.py")), Path::new("."));
    }
}
