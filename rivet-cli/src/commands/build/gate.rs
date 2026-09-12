//! The post-generation Gauntlet step for `rivet build`.
//!
//! Rivet's own DSL-level gates are the Verifier; they run between parse and
//! generate and they inspect the Python module. This step is a different
//! layer: it invokes the standalone `gauntlet` CLI, a language-agnostic
//! codebase-level orchestrator, on the Rust crate Rivet just wrote to disk
//! and before cargo compiles it.
//!
//! The binary is detected at runtime and is never a dependency. A host
//! without it skips the step. `gauntlet check` reports through its exit
//! code:
//!
//! - `0` clean: the build continues.
//! - `2` blocking: the build stops before it compiles.
//! - `3` advisory: the build prints the findings and continues.
//!
//! Any other non-zero code is a failure of the gate itself, so the build
//! stops and names the code. A gate that breaks must not look like a gate
//! that passed.

use crate::diagnostic::Diagnostic;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The command name of the standalone Gauntlet CLI.
pub(super) const GAUNTLET_BINARY: &str = "gauntlet";

/// The line the build prints when the CLI is not on `PATH`.
pub(super) const SKIP_NOTICE: &str =
    "[rivet] gauntlet CLI not found; skipping post-generation gate";

/// The exit code `gauntlet check` uses for blocking findings.
const EXIT_BLOCKING: i32 = 2;

/// The exit code `gauntlet check` uses for advisory findings.
const EXIT_ADVISORY: i32 = 3;

/// What the post-generation step did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Outcome {
    /// The CLI ran and reported nothing.
    Clear,
    /// The CLI reported advisory findings; the build continues.
    Advisory,
    /// The CLI reported blocking findings; the caller stops the build.
    Blocked,
}

/// Run the gate against the CLI on `PATH`.
///
/// A missing binary is not an error: the step prints [`SKIP_NOTICE`] to
/// stderr and the build continues. Blocking findings return one diagnostic
/// that stops the build before it compiles.
pub(super) fn enforce(crate_dir: &Path) -> Result<(), Vec<Diagnostic>> {
    let binary = std::env::var_os("PATH").and_then(|path| locate_in(&path, GAUNTLET_BINARY));
    enforce_with(crate_dir, binary.as_deref())
}

/// The gate body. `None` means the CLI is not on `PATH`.
///
/// Split out so tests can drive a stub binary without touching the process
/// environment.
fn enforce_with(crate_dir: &Path, binary: Option<&Path>) -> Result<(), Vec<Diagnostic>> {
    let Some(binary) = binary else {
        eprintln!("{SKIP_NOTICE}");
        return Ok(());
    };
    match invoke(binary, crate_dir)? {
        Outcome::Clear | Outcome::Advisory => Ok(()),
        Outcome::Blocked => Err(vec![blocked_diagnostic(crate_dir)]),
    }
}

/// The diagnostic that stops the build when the CLI reports blockers.
fn blocked_diagnostic(crate_dir: &Path) -> Diagnostic {
    Diagnostic::blocker(
        "E2018",
        format!(
            "the gauntlet CLI reported blocking findings on {}",
            crate_dir.display()
        ),
        "fix the findings in the gauntlet output above, or pass `--no-gauntlet` to skip the post-generation gate, then rerun `rivet build`",
    )
}

/// Find `binary` in a `PATH`-style list.
///
/// The lookup mirrors what a shell does: split on the platform separator and
/// take the first entry that names an existing file.
fn locate_in(path: &OsStr, binary: &str) -> Option<PathBuf> {
    std::env::split_paths(path)
        .filter(|dir| !dir.as_os_str().is_empty())
        .map(|dir| dir.join(binary))
        .find(|candidate| candidate.is_file())
}

/// Run `gauntlet check --tier=standard --target=<crate_dir>`.
///
/// The CLI writes its findings to stdout or stderr; the step forwards both so
/// a human or an agent sees them beside the build's own output.
fn invoke(binary: &Path, crate_dir: &Path) -> Result<Outcome, Vec<Diagnostic>> {
    let output = Command::new(binary)
        .arg("check")
        .arg("--tier=standard")
        .arg(format!("--target={}", crate_dir.display()))
        .output()
        .map_err(|err| {
            vec![Diagnostic::blocker(
                "E2018",
                format!("failed to run the gauntlet CLI: {err}"),
                "make the `gauntlet` binary on your PATH executable, or pass `--no-gauntlet` to skip the post-generation gate, then rerun `rivet build`",
            )]
        })?;

    print_findings(&output);
    match output.status.code() {
        Some(0) => Ok(Outcome::Clear),
        Some(EXIT_BLOCKING) => Ok(Outcome::Blocked),
        Some(EXIT_ADVISORY) => Ok(Outcome::Advisory),
        Some(code) => Err(vec![unexpected_exit(format!(
            "the gauntlet CLI exited with code {code}"
        ))]),
        None => Err(vec![unexpected_exit(
            "the gauntlet CLI was killed by a signal".to_string(),
        )]),
    }
}

/// The diagnostic for a gate that failed in a way its contract does not name.
fn unexpected_exit(message: String) -> Diagnostic {
    Diagnostic::blocker(
        "E2018",
        message,
        "run `gauntlet check --tier=standard` on the generated crate to see the failure, or pass `--no-gauntlet` to skip the post-generation gate",
    )
}

/// Forward the CLI's own output to the build's stderr.
fn print_findings(output: &std::process::Output) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stdout.trim().is_empty() {
        eprintln!("[rivet] gauntlet findings:\n{}", stdout.trim_end());
    }
    if !stderr.trim().is_empty() {
        eprintln!("{}", stderr.trim_end());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::ScratchDir;

    /// A stub `gauntlet` that prints `message` to stderr and exits `code`.
    #[cfg(unix)]
    fn stub(dir: &ScratchDir, code: i32, message: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;

        let path = dir.join(GAUNTLET_BINARY);
        std::fs::write(
            &path,
            format!("#!/bin/sh\necho \"{message}\" >&2\nexit {code}\n"),
        )
        .expect("write the stub");
        let mut permissions = std::fs::metadata(&path)
            .expect("stat the stub")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&path, permissions).expect("make the stub executable");
        path
    }

    #[test]
    fn locate_in_reports_a_missing_binary() {
        assert_eq!(
            locate_in(OsStr::new("/nonexistent-rivet-bin-dir"), GAUNTLET_BINARY),
            None
        );
    }

    #[test]
    fn locate_in_finds_a_present_binary() {
        let dir = ScratchDir::new("gate-locate");
        std::fs::write(dir.join(GAUNTLET_BINARY), "#!/bin/sh\n").expect("write a file");
        assert_eq!(
            locate_in(dir.as_ref().as_os_str(), GAUNTLET_BINARY),
            Some(dir.join(GAUNTLET_BINARY))
        );
    }

    #[test]
    fn a_missing_cli_skips_the_gate() {
        let dir = ScratchDir::new("gate-skip");
        assert!(enforce_with(dir.path(), None).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn exit_two_is_blocking() {
        let dir = ScratchDir::new("gate-blocking");
        let binary = stub(&dir, EXIT_BLOCKING, "complexity above the limit");
        let diagnostics = enforce_with(dir.path(), Some(&binary)).expect_err("exit 2 blocks");
        assert_eq!(diagnostics[0].error_code, "E2018");
        assert!(!diagnostics[0].suggested_fix.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn a_clean_or_advisory_run_lets_the_build_continue() {
        let dir = ScratchDir::new("gate-clear");
        let clean = stub(&dir, 0, "nothing to report");
        assert_eq!(
            invoke(&clean, dir.path()).expect("the stub runs"),
            Outcome::Clear
        );
        assert!(enforce_with(dir.path(), Some(&clean)).is_ok());

        let advisory = stub(&dir, EXIT_ADVISORY, "formatting drift");
        assert_eq!(
            invoke(&advisory, dir.path()).expect("the stub runs"),
            Outcome::Advisory
        );
        assert!(enforce_with(dir.path(), Some(&advisory)).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn an_unknown_exit_code_stops_the_gate() {
        let dir = ScratchDir::new("gate-unknown");
        let binary = stub(&dir, 7, "unexpected");
        let diagnostics =
            enforce_with(dir.path(), Some(&binary)).expect_err("code 7 is not a verdict");
        assert_eq!(diagnostics[0].error_code, "E2018");
    }

    #[test]
    fn a_missing_binary_file_is_not_reported_as_present() {
        let dir = ScratchDir::new("gate-absent");
        assert_eq!(locate_in(dir.as_ref().as_os_str(), GAUNTLET_BINARY), None);
    }
}
