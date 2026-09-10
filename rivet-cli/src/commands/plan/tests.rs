use super::*;
use crate::diagnostic::Severity;
use std::fs;
use std::path::{Path, PathBuf};

const PING_MODULE: &str = "from rivet import api\n\n@api.get(\"/ping\", stories=[\"US-1\"])\ndef ping() -> dict:\n    return {\"status\": \"pong\"}\n";
const HEALTH_MODULE: &str = "from rivet import api\n\n@api.get(\"/ping\", stories=[\"US-1\"])\ndef ping() -> dict:\n    return {\"status\": \"pong\"}\n\n@api.get(\"/health\", stories=[\"US-42\"])\ndef health() -> dict:\n    return {\"status\": \"ok\"}\n";

/// A scratch git project under the temp dir, unique per tag and process.
fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rivet-plan-test-{}-{tag}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create fixture dir");
    dir
}

fn git_ok(dir: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// A committed fixture: local git identity, `.gitignore` for `.rivet/`
/// and `generated/`, and an app.py with only the ping route.
fn committed_fixture(tag: &str) -> (PathBuf, PathBuf) {
    let dir = scratch(tag);
    git_ok(&dir, &["init", "-q"]);
    git_ok(&dir, &["config", "user.name", "Rivet Test"]);
    git_ok(&dir, &["config", "user.email", "rivet@test.local"]);
    fs::write(dir.join(".gitignore"), ".rivet/\ngenerated/\n").expect("write .gitignore");
    let app = dir.join("app.py");
    fs::write(&app, PING_MODULE).expect("write app.py");
    git_ok(&dir, &["add", "."]);
    git_ok(&dir, &["commit", "-q", "-m", "initial"]);
    (dir, app)
}

/// The replacement module path, OUTSIDE the repo so it cannot dirty the
/// working tree the preflight inspects.
fn replacement(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "rivet-plan-replacement-{tag}-{}.py",
        std::process::id()
    ))
}

#[test]
fn from_file_converges_commits_and_compiles() {
    let (dir, app) = committed_fixture("from");
    let new_app = replacement("from");
    fs::write(&new_app, HEALTH_MODULE).expect("write replacement module");

    run_plan("add a health route US-42", &app, Some(&new_app), false).expect("plan succeeds");

    assert_eq!(
        git_ok(&dir, &["branch", "--show-current"]),
        "rivet/plan/add-a-health-route-us-42"
    );
    let module = fs::read_to_string(&app).expect("read app.py");
    assert!(module.contains("@api.get(\"/health\""), "{module}");
    assert!(module.contains("US-42"), "{module}");
    let spec = fs::read_to_string(dir.join("SPEC.md")).expect("SPEC.md written");
    assert!(spec.contains("Plan: add a health route US-42"), "{spec}");
    let subject = git_ok(&dir, &["log", "--oneline", "-1"]);
    assert!(subject.contains("plan: add a health route"), "{subject}");
    run_build(&app).expect("the generated crate compiles");
    let _ = fs::remove_file(&new_app);
    fs::remove_dir_all(&dir).expect("clean up fixture");
}

#[test]
fn dirty_working_tree_refuses_to_plan() {
    let (dir, app) = committed_fixture("dirty");
    let new_app = replacement("dirty");
    fs::write(&app, format!("{PING_MODULE}# dirty\n")).expect("dirty the tree");

    let diagnostics = run_plan("add a health route US-42", &app, Some(&new_app), false)
        .expect_err("a dirty tree must refuse to plan");
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].severity, Severity::Blocker);
    assert!(
        diagnostics[0].message.contains("dirty"),
        "{}",
        diagnostics[0].message
    );
    let _ = fs::remove_file(&new_app);
    fs::remove_dir_all(&dir).expect("clean up fixture");
}
