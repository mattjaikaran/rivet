//! Delivery helpers for `rivet /plan`: git preflight and plumbing, SPEC.md
//! writing, and the `gh` PR open on `--push`.

use super::provider;
use crate::config::RivetConfig;
use crate::diagnostic::{Diagnostic, Severity};
use crate::parser::python::ParsedModule;
use crate::verifier;
use serde_json::Value;
use std::path::Path;

/// Refuse to plan outside a git checkout or over a dirty working tree.
pub(super) fn git_preflight(project_dir: &Path) -> Result<(), Vec<Diagnostic>> {
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
pub(super) fn branch_name(story: &str) -> String {
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

pub(super) fn run_git(project_dir: &Path, args: &[&str]) -> Result<String, String> {
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

pub(super) fn err(
    code: &str,
    message: impl Into<String>,
    fix: impl Into<String>,
) -> Vec<Diagnostic> {
    vec![Diagnostic::blocker(code, message, fix)]
}

/// E3014 summary plus the diagnostics that explain the underlying failure.
pub(super) fn failed(what: &str, underlying: Vec<Diagnostic>) -> Vec<Diagnostic> {
    let mut out = err(
        "E3014",
        format!("rivet /plan failed to {what}: the module still fails after retries"),
        "make the story more precise and run rivet /plan again, or write the replacement module yourself and pass it with --from FILE",
    );
    out.extend(underlying);
    out
}

pub(super) fn verifier_blockers(module: &ParsedModule, config: &RivetConfig) -> Vec<Diagnostic> {
    verifier::run_verifier(module, &config.verifier)
        .into_iter()
        .filter(|finding| finding.severity == Severity::Blocker)
        .collect()
}

/// One provider repair round: ask again with the failed diagnostics.
pub(super) async fn repair(
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

pub(super) fn feedback_json(diagnostics: &[Diagnostic]) -> String {
    let values: Vec<Value> = diagnostics.iter().map(Diagnostic::to_json_value).collect();
    Value::Array(values).to_string()
}

/// The letter grade from the audit JSON, `?` when the shape is unexpected.
pub(super) fn grade_from_audit(audit: &str) -> String {
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
pub(super) fn generated_summary(story: &str, module: &ParsedModule, grade: &str) -> String {
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

pub(super) fn write_text(path: &Path, contents: &str) -> Result<(), Vec<Diagnostic>> {
    std::fs::write(path, contents).map_err(|cause| {
        err(
            "E1008",
            format!("failed to write {}: {cause}", path.display()),
            "fix the filesystem error above, then run rivet /plan again",
        )
    })
}

/// Open the PR with `gh` on `--push`; failures print a warning only.
pub(super) fn open_pr(project_dir: &Path, branch: &str) {
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
