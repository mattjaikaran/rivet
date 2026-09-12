use super::*;
use crate::diagnostic::Severity;
use crate::test_support::ScratchDir;
use std::fs;
use std::path::{Path, PathBuf};

const PING_MODULE: &str = "from rivet import api\n\n@api.get(\"/ping\", stories=[\"US-1\"])\ndef ping() -> dict:\n    return {\"status\": \"pong\"}\n";
const HEALTH_MODULE: &str = "from rivet import api\n\n@api.get(\"/ping\", stories=[\"US-1\"])\ndef ping() -> dict:\n    return {\"status\": \"pong\"}\n\n@api.get(\"/health\", stories=[\"US-42\"])\ndef health() -> dict:\n    return {\"status\": \"ok\"}\n";

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
fn committed_fixture(tag: &str) -> (ScratchDir, PathBuf, PathBuf) {
    let guard = ScratchDir::new(&format!("plan-{tag}"));
    let repo = guard.join("repo");
    fs::create_dir_all(&repo).expect("create repo dir");
    git_ok(&repo, &["init", "-q"]);
    git_ok(&repo, &["config", "user.name", "Rivet Test"]);
    git_ok(&repo, &["config", "user.email", "rivet@test.local"]);
    fs::write(repo.join(".gitignore"), ".rivet/\ngenerated/\n").expect("write .gitignore");
    let app = repo.join("app.py");
    fs::write(&app, PING_MODULE).expect("write app.py");
    git_ok(&repo, &["add", "."]);
    git_ok(&repo, &["commit", "-q", "-m", "initial"]);
    (guard, repo, app)
}

#[test]
fn from_file_converges_commits_and_compiles() {
    let (guard, repo, app) = committed_fixture("from");
    let new_app = guard.join("new_app.py");
    fs::write(&new_app, HEALTH_MODULE).expect("write replacement module");

    run_plan("add a health route US-42", &app, Some(&new_app), false).expect("plan succeeds");

    assert_eq!(
        git_ok(&repo, &["branch", "--show-current"]),
        "rivet/plan/add-a-health-route-us-42"
    );
    let module = fs::read_to_string(&app).expect("read app.py");
    assert!(module.contains("@api.get(\"/health\""), "{module}");
    assert!(module.contains("US-42"), "{module}");
    let spec = fs::read_to_string(repo.join("SPEC.md")).expect("SPEC.md written");
    assert!(spec.contains("Plan: add a health route US-42"), "{spec}");
    let subject = git_ok(&repo, &["log", "--oneline", "-1"]);
    assert!(subject.contains("plan: add a health route"), "{subject}");
    // Production `rivet plan` builds with the post-generation gate skipped,
    // so the test mirrors that path and never depends on whether the
    // standalone `gauntlet` binary is installed.
    run_build(&app, BuildTarget::Native, true).expect("the generated crate compiles");
}

#[test]
fn dirty_working_tree_refuses_to_plan() {
    let (guard, _repo, app) = committed_fixture("dirty");
    let new_app = guard.join("new_app.py");
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
}
