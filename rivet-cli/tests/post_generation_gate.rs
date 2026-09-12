//! End-to-end tests for the post-generation Gauntlet step.
//!
//! The step is a different layer from the Verifier: it invokes the standalone
//! `gauntlet` CLI on the crate `rivet build` wrote, and the CLI reports
//! through its exit code. These tests drive the real `rivet` binary as a
//! child process, so they control `PATH` exactly: a stub on `PATH` proves the
//! blocking path, a `PATH` without the binary proves the skip path.
//!
//! `cargo` is stubbed too. The gate runs before cargo compiles, so a shim
//! records whether compilation was reached, and the tests stay hermetic and
//! fast instead of compiling an axum crate.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// A module that passes the Verifier: one route, one story ID.
const FIXTURE_APP: &str = "from rivet import api\n\n@api.get(\"/ping\", stories=[\"US-001\"])\ndef ping() -> dict:\n    return {\"status\": \"pong\"}\n";

/// The `rivet` binary under test, built by cargo for this integration test.
const RIVET: &str = env!("CARGO_BIN_EXE_rivet");

/// A scratch directory that removes itself on drop, including on a panic.
struct ScratchDir(PathBuf);

impl ScratchDir {
    /// Create a clean `<temp_dir>/rivet-<name>`.
    fn new(name: &str) -> ScratchDir {
        let path = std::env::temp_dir().join(format!("rivet-{name}"));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create the scratch directory");
        ScratchDir(path)
    }

    /// The directory path.
    fn path(&self) -> &Path {
        &self.0
    }

    /// Write `app.py` and `rivet.toml` into the project directory.
    fn write_project(&self) {
        fs::write(self.0.join("app.py"), FIXTURE_APP).expect("write app.py");
        fs::write(
            self.0.join("rivet.toml"),
            "[project]\nname = \"gatefixture\"\n",
        )
        .expect("write rivet.toml");
    }

    /// A `tools` directory beside the project, for the stubbed `PATH`.
    fn tools(&self) -> PathBuf {
        let tools = self.0.join("tools");
        fs::create_dir_all(&tools).expect("create the tools directory");
        tools
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Write an executable `#!/bin/sh` stub named `name`.
///
/// The stub prints `message` to stderr and exits `code`, which is exactly the
/// contract the gate reads.
fn write_stub(dir: &Path, name: &str, code: i32, message: &str) -> PathBuf {
    write_script(dir, name, &format!("echo \"{message}\" >&2\nexit {code}"))
}

/// Write an executable `#!/bin/sh` file named `name` with `body`.
fn write_script(dir: &Path, name: &str, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let path = dir.join(name);
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("write the stub");
    let mut permissions = fs::metadata(&path).expect("stat the stub").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&path, permissions).expect("make the stub executable");
    path
}

/// Run `rivet build` on the scratch project with `PATH` set to `bin_dirs`.
fn build_with_path(dir: &ScratchDir, bin_dirs: &[&Path], extra_args: &[&str]) -> Output {
    let path = std::env::join_paths(bin_dirs).expect("join the stub PATH");
    let mut command = Command::new(RIVET);
    command.arg("build");
    command.args(extra_args);
    command
        .arg(dir.path().join("app.py"))
        .env("PATH", path)
        .output()
        .expect("run the rivet binary")
}

#[test]
#[cfg(unix)]
fn build_skips_gauntlet_when_absent() {
    let dir = ScratchDir::new("gate-absent-build");
    dir.write_project();
    // A PATH with `cargo` but no `gauntlet`: the gate must skip, and the
    // build must reach a successful compile.
    let tools = dir.tools();
    write_stub(&tools, "cargo", 0, "");

    let output = build_with_path(&dir, &[&tools], &[]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        output.status.success(),
        "the build must succeed without the gauntlet CLI:\n{stderr}"
    );
    assert!(
        stderr.contains("[rivet] gauntlet CLI not found; skipping post-generation gate"),
        "the skip notice names the missing CLI and the step:\n{stderr}"
    );
    assert!(
        stdout.contains("Build succeeded."),
        "the build reaches the compile step:\n{stdout}"
    );
    assert!(
        dir.path().join("generated/Cargo.toml").exists(),
        "the crate is written before the gate runs"
    );
}

#[test]
#[cfg(unix)]
fn build_aborts_on_gauntlet_blocker() {
    let dir = ScratchDir::new("gate-blocker-build");
    dir.write_project();
    // A stub `gauntlet` that reports blocking findings, and a `cargo` that
    // records whether it was ever reached.
    let tools = dir.tools();
    write_stub(&tools, "gauntlet", 2, "E9001 complexity 12 above the limit");
    let marker = tools.join("cargo-ran");
    write_script(&tools, "cargo", &format!(": > '{}'", marker.display()));

    let output = build_with_path(&dir, &[&tools], &[]);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !output.status.success(),
        "a blocking gauntlet verdict must fail the build"
    );
    assert!(
        stderr.contains("E9001 complexity 12 above the limit"),
        "the gate forwards the CLI's own findings:\n{stderr}"
    );
    assert!(
        stderr.contains("blocking findings"),
        "the failure names the gate's verdict:\n{stderr}"
    );
    assert!(
        !marker.exists(),
        "the build must abort before it compiles the crate"
    );
}

#[test]
#[cfg(unix)]
fn no_gauntlet_skips_the_step() {
    let dir = ScratchDir::new("gate-flag-build");
    dir.write_project();
    // The stub would block, so a passing build proves the flag skipped it.
    let tools = dir.tools();
    write_stub(&tools, "gauntlet", 2, "E9001 complexity 12 above the limit");
    // The cargo stub records that it ran, which proves the blocking test's
    // assertion is not vacuous: without the flag, cargo is reached.
    let marker = tools.join("cargo-ran");
    write_script(&tools, "cargo", &format!(": > '{}'", marker.display()));

    let output = build_with_path(&dir, &[&tools], &["--no-gauntlet"]);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "`--no-gauntlet` must skip the step and build:\n{stderr}"
    );
    assert!(
        !stderr.contains("E9001") && !stderr.contains("gauntlet CLI not found"),
        "the flag skips the step instead of reporting or running it:\n{stderr}"
    );
    assert!(
        marker.exists(),
        "the skipped gate must let the build reach the compile step"
    );
}

#[test]
#[cfg(unix)]
fn a_deprecated_gauntlet_section_warns_and_still_builds() {
    let dir = ScratchDir::new("gate-config-shim");
    dir.write_project();
    // The old section name must still load, and it must say so on stderr.
    fs::write(
        dir.path().join("rivet.toml"),
        "[project]\nname = \"gatefixture\"\n\n[gauntlet]\nmax_complexity = 5\n",
    )
    .expect("write the legacy rivet.toml");
    let tools = dir.tools();
    write_stub(&tools, "cargo", 0, "");

    let output = build_with_path(&dir, &[&tools], &[]);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "the legacy section must not fail the build:\n{stderr}"
    );
    assert!(
        stderr.contains("uses the deprecated `[gauntlet]` section"),
        "the loader names the deprecated section and its replacement:\n{stderr}"
    );
    assert!(
        stderr.contains("rename it to `[verifier]`"),
        "the warning names the replacement section:\n{stderr}"
    );
}

#[test]
#[cfg(unix)]
fn a_verifier_section_prints_no_deprecation_warning() {
    let dir = ScratchDir::new("gate-config-current");
    dir.write_project();
    fs::write(
        dir.path().join("rivet.toml"),
        "[project]\nname = \"gatefixture\"\n\n[verifier]\nmax_complexity = 5\n",
    )
    .expect("write the current rivet.toml");
    let tools = dir.tools();
    write_stub(&tools, "cargo", 0, "");

    let output = build_with_path(&dir, &[&tools], &[]);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "the current section must build:\n{stderr}"
    );
    assert!(
        !stderr.contains("deprecated"),
        "the current section must not warn:\n{stderr}"
    );
}
