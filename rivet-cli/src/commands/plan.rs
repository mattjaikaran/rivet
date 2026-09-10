//! `rivet /plan "<story>" [app.py] [--from FILE] [--push]`: spec-driven
//! auto-PR command (phase 3, pillar 09). Cuts a `rivet/plan/<slug>` branch,
//! converges on a replacement module (provider or `--from FILE`, one repair
//! round), verifies the crate via [`crate::commands::build::run_build`],
//! writes SPEC.md, commits, and opens the PR with `gh` on `--push`.
//!
//! Codes: E1008 file/config, E3012 provider/env, E3014 converge-or-verify, E3015 git.

use crate::commands::audit::audit_json;
use crate::commands::build::run_build;
use crate::commands::session::render_context;
use crate::config::RivetConfig;
use crate::diagnostic::{Diagnostic, Severity};
use crate::gauntlet;
use crate::parser::python::{ParsedModule, parse_python_module};
use crate::store;
use serde_json::Value;
use std::path::Path;

mod provider;

const MAX_ATTEMPTS: usize = 2;

/// Run the plan pipeline; failures come back as structured diagnostics.
pub fn run_plan(
    story: &str,
    app_file: &Path,
    from: Option<&Path>,
    push: bool,
) -> Result<(), Vec<Diagnostic>> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|cause| {
            err(
                "E3012",
                format!("failed to start the async runtime: {cause}"),
                "retry `rivet /plan`; report the error if it keeps happening",
            )
        })?;
    runtime.block_on(plan_async(story, app_file, from, push))
}

async fn plan_async(
    story: &str,
    app_file: &Path,
    from: Option<&Path>,
    push: bool,
) -> Result<(), Vec<Diagnostic>> {
    let project_dir = store::project_dir_for(app_file);
    let config = RivetConfig::load(&project_dir).map_err(|message| {
        err(
            "E1008",
            message,
            "fix the rivet.toml error above, then run rivet /plan again",
        )
    })?;
    if !app_file.exists() {
        return Err(err(
            "E1008",
            format!("{} not found", app_file.display()),
            "write a `from rivet import api` module, then run rivet /plan again",
        ));
    }

    git_preflight(&project_dir)?;

    let branch = branch_name(story);
    run_git(&project_dir, &["checkout", "-b", branch.as_str()]).map_err(|reason| {
        err(
            "E3015",
            format!("could not create branch {branch}: {reason}"),
            "switch off or delete the existing branch, or tell a different story",
        )
    })?;

    let context = render_context(app_file)?;

    let provider_cfg = if from.is_some() {
        None
    } else {
        Some(provider::from_env()?)
    };
    let mut plan_spec: Option<String> = None;
    let mut source = match (from, provider_cfg.as_ref()) {
        (Some(path), _) => std::fs::read_to_string(path).map_err(|read_err| {
            err(
                "E1008",
                format!("failed to read {}: {read_err}", path.display()),
                "check the --from path and run rivet /plan again",
            )
        })?,
        (None, Some(cfg)) => {
            let response = provider::generate(cfg, story, &context).await?;
            plan_spec = Some(response.spec);
            response.module
        }
        (None, None) => {
            return Err(err(
                "E3012",
                "no module source: pass --from FILE or configure a provider",
                "set RIVET_PLAN_API_KEY or point RIVET_PLAN_BASE_URL at a local server, then run rivet /plan again",
            ));
        }
    };

    let module_name = app_file
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("app")
        .to_string();
    let file_label = app_file.display().to_string();

    // Converge: parse + Gauntlet until clean. `--from` fails immediately; the provider gets one feedback round.
    let mut attempt = 0;
    let mut module = loop {
        attempt += 1;
        match parse_python_module(&source, &module_name, &file_label) {
            Err(parse_error) if from.is_some() => return Err(vec![parse_error]),
            Err(parse_error) if attempt >= MAX_ATTEMPTS => {
                return Err(failed("converge", vec![parse_error]));
            }
            Err(parse_error) => {
                source = repair(
                    &provider_cfg,
                    story,
                    &context,
                    &feedback_json(&[parse_error]),
                    &mut plan_spec,
                )
                .await?;
            }
            Ok(parsed) => {
                let blockers = gauntlet_blockers(&parsed, &config);
                if blockers.is_empty() {
                    break parsed;
                }
                if from.is_some() || attempt >= MAX_ATTEMPTS {
                    return Err(if from.is_some() {
                        blockers
                    } else {
                        failed("converge", blockers)
                    });
                }
                source = repair(
                    &provider_cfg,
                    story,
                    &context,
                    &feedback_json(&blockers),
                    &mut plan_spec,
                )
                .await?;
            }
        }
    };

    write_text(app_file, &source)?;
    if let Err(build_diagnostics) = run_build(app_file) {
        if from.is_some() {
            return Err(failed("compile", build_diagnostics));
        }
        source = repair(
            &provider_cfg,
            story,
            &context,
            &feedback_json(&build_diagnostics),
            &mut plan_spec,
        )
        .await?;
        let revised = match parse_python_module(&source, &module_name, &file_label) {
            Err(parse_error) => return Err(failed("compile", vec![parse_error])),
            Ok(module) => module,
        };
        let blockers = gauntlet_blockers(&revised, &config);
        if !blockers.is_empty() {
            return Err(failed("compile", blockers));
        }
        write_text(app_file, &source)?;
        if let Err(build_diagnostics) = run_build(app_file) {
            return Err(failed("compile", build_diagnostics));
        }
        module = revised;
    }

    let audit = audit_json(app_file)?;
    let grade = grade_from_audit(&audit);
    let spec_md = match plan_spec {
        Some(spec) if !spec.trim().is_empty() => spec,
        _ => generated_summary(story, &module, &grade),
    };
    write_text(&project_dir.join("SPEC.md"), &spec_md)?;

    let relative_app = app_file
        .strip_prefix(&project_dir)
        .unwrap_or(app_file)
        .to_string_lossy();
    run_git(&project_dir, &["add", relative_app.as_ref(), "SPEC.md"]).map_err(|reason| {
        err(
            "E3015",
            format!("git add failed: {reason}"),
            "resolve the git error above, then run rivet /plan again",
        )
    })?;
    run_git(&project_dir, &["commit", "-m", &format!("plan: {story}")]).map_err(|reason| {
        let detail = if reason.trim().is_empty() { "git commit failed without a message".to_string() } else { reason };
        err("E3015", detail, "set your git identity with `git config user.name \"...\"` and `git config user.email \"...\"`, then run rivet /plan again")
    })?;

    let stat = run_git(&project_dir, &["show", "--stat", "--oneline", "HEAD"])
        .unwrap_or_else(|_| "(no diff stat)".to_string());
    println!("# {story}\n\nAudit grade: {grade}\n\n## Changes\n\n{stat}");
    if push {
        open_pr(&project_dir, &branch);
    }
    println!("Branch: {branch}");
    println!("Files committed: {relative_app}, SPEC.md");
    println!("Audit grade: {grade}");
    if !push {
        println!("Push and open the PR: git push -u origin {branch} && gh pr create --fill");
    }
    Ok(())
}

/// Refuse to plan outside a git checkout or over a dirty working tree.
fn git_preflight(project_dir: &Path) -> Result<(), Vec<Diagnostic>> {
    if run_git(project_dir, &["rev-parse", "--is-inside-work-tree"]).as_deref() != Ok("true") {
        return Err(err(
            "E3015",
            "the project is not a git checkout; run git init and commit first",
            "run `git init`, make an initial commit, then run rivet /plan again",
        ));
    }
    let status = run_git(project_dir, &["status", "--porcelain"]).map_err(|reason| {
        err(
            "E3015",
            format!("git status failed: {reason}"),
            "resolve the git error above, then run rivet /plan again",
        )
    })?;
    if status.is_empty() {
        return Ok(());
    }
    Err(err(
        "E3015",
        format!("the working tree is dirty:\n{status}"),
        "commit or stash the changes above, then run rivet /plan again",
    ))
}

/// The `rivet/plan/<slug>` branch for a story.
fn branch_name(story: &str) -> String {
    format!("rivet/plan/{}", slugify(story))
}

/// Story to a git-safe slug: lowercase, non-alphanumerics to `-`, collapsed
/// and trimmed to 40 characters; `plan` when nothing survives.
fn slugify(story: &str) -> String {
    let mut out = String::new();
    let mut previous_dash = false;
    for ch in story.to_lowercase().chars() {
        let ch = if ch.is_ascii_alphanumeric() || ch == '-' {
            ch
        } else {
            '-'
        };
        if !(ch == '-' && previous_dash) {
            out.push(ch);
        }
        previous_dash = ch == '-';
    }
    let mut out: String = out.trim_matches('-').chars().take(40).collect();
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        "plan".to_string()
    } else {
        out
    }
}

fn run_git(project_dir: &Path, args: &[&str]) -> Result<String, String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(project_dir)
        .args(args)
        .output()
        .map_err(|err| format!("failed to run git: {err}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        return Err(if stderr.is_empty() { stdout } else { stderr });
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn err(code: &str, message: impl Into<String>, fix: impl Into<String>) -> Vec<Diagnostic> {
    vec![Diagnostic::blocker(code, message, fix)]
}

/// E3014 summary plus the diagnostics that explain the underlying failure.
fn failed(what: &str, underlying: Vec<Diagnostic>) -> Vec<Diagnostic> {
    let mut out = err(
        "E3014",
        format!("rivet /plan failed to {what}: the module still fails after retries"),
        "make the story more precise and run rivet /plan again, or write the replacement module yourself and pass it with --from FILE",
    );
    out.extend(underlying);
    out
}

fn gauntlet_blockers(module: &ParsedModule, config: &RivetConfig) -> Vec<Diagnostic> {
    gauntlet::run_gauntlet(module, &config.gauntlet)
        .into_iter()
        .filter(|finding| finding.severity == Severity::Blocker)
        .collect()
}

/// One provider repair round: ask again with the failed diagnostics.
async fn repair(
    provider_cfg: &Option<provider::ProviderConfig>,
    story: &str,
    context: &str,
    feedback: &str,
    plan_spec: &mut Option<String>,
) -> Result<String, Vec<Diagnostic>> {
    let cfg = provider_cfg.as_ref().ok_or_else(|| {
        err(
            "E3012",
            "a provider repair was requested but none is configured",
            "pass --from FILE or configure the provider, then run rivet /plan again",
        )
    })?;
    let response = provider::generate_with_feedback(cfg, story, context, feedback).await?;
    *plan_spec = Some(response.spec);
    Ok(response.module)
}

fn feedback_json(diagnostics: &[Diagnostic]) -> String {
    let values: Vec<Value> = diagnostics.iter().map(Diagnostic::to_json_value).collect();
    Value::Array(values).to_string()
}

/// The letter grade from the audit JSON, `?` when the shape is unexpected.
fn grade_from_audit(audit: &str) -> String {
    serde_json::from_str::<Value>(audit)
        .ok()
        .and_then(|value| {
            value
                .get("grade")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| "?".to_string())
}

/// Generated SPEC.md without provider spec text: routes and the audit grade.
fn generated_summary(story: &str, module: &ParsedModule, grade: &str) -> String {
    let mut out = format!("# Plan: {story}\n\n## Result\n\nRoutes:\n");
    for route in &module.blueprint.routes {
        out.push_str(&format!("- `{} {}`", route.method.as_str(), route.path));
        if !route.stories.is_empty() {
            out.push_str(&format!(" (stories: {})", route.stories.join(", ")));
        }
        out.push('\n');
    }
    if module.blueprint.routes.is_empty() {
        out.push_str("- none\n");
    }
    out.push_str(&format!("\nAudit grade: {grade}\n"));
    out
}

fn write_text(path: &Path, contents: &str) -> Result<(), Vec<Diagnostic>> {
    std::fs::write(path, contents).map_err(|cause| {
        err(
            "E1008",
            format!("failed to write {}: {cause}", path.display()),
            "fix the filesystem error above, then run rivet /plan again",
        )
    })
}

/// Open the PR with `gh` on `--push`; failures print a warning only.
fn open_pr(project_dir: &Path, branch: &str) {
    match std::process::Command::new("gh")
        .current_dir(project_dir)
        .args(["pr", "create", "--fill"])
        .output()
    {
        Ok(output) if output.status.success() => {
            let text = String::from_utf8_lossy(&output.stdout);
            if text.trim().is_empty() {
                println!("PR created.");
            } else {
                println!("{}", text.trim());
            }
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let detail = if stderr.is_empty() { stdout } else { stderr };
            if detail.is_empty() {
                println!("warning: gh pr create failed: no output");
            } else {
                println!("warning: gh pr create failed: {detail}");
            }
        }
        Err(spawn_err) if spawn_err.kind() == std::io::ErrorKind::NotFound => println!(
            "gh is not installed; open the PR manually with `git push -u origin {branch}` and `gh pr create --fill`"
        ),
        Err(spawn_err) => println!("could not run gh pr create: {spawn_err}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::{Path, PathBuf};

    const PING_MODULE: &str = "from rivet import api\n\n@api.get(\"/ping\", stories=[\"US-1\"])\ndef ping() -> dict:\n    return {\"status\": \"pong\"}\n";
    const HEALTH_MODULE: &str = "from rivet import api\n\n@api.get(\"/ping\", stories=[\"US-1\"])\ndef ping() -> dict:\n    return {\"status\": \"pong\"}\n\n@api.get(\"/health\", stories=[\"US-42\"])\ndef health() -> dict:\n    return {\"status\": \"ok\"}\n";

    /// A scratch git project under the temp dir, unique per tag and process.
    fn scratch(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("rivet-plan-test-{}-{tag}", std::process::id()));
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
}
