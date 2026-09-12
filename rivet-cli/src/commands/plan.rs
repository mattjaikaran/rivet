//! `rivet /plan "<story>" [app.py] [--from FILE] [--push]`: spec-driven
//! auto-PR command (phase 3, pillar 09). Cuts a `rivet/plan/<slug>` branch,
//! converges on a replacement module (provider or `--from FILE`, one repair
//! round), verifies the crate via [`crate::commands::build::run_build`],
//! writes SPEC.md, commits, and opens the PR with `gh` on `--push`.
//!
//! Codes: E1008 file/config, E3012 provider/env, E3014 converge-or-verify, E3015 git.

use crate::commands::audit::audit_json;
use crate::commands::build::{BuildTarget, run_build};
use crate::commands::session::render_context;
use crate::config::RivetConfig;
use crate::diagnostic::Diagnostic;
use crate::parser::python::parse_python_module;
use crate::store;
use std::path::Path;

mod deliver;
mod provider;

use deliver::{
    branch_name, err, failed, feedback_json, generated_summary, git_preflight, grade_from_audit,
    open_pr, repair, run_git, verifier_blockers, write_text,
};

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

    // Converge: parse + Verifier until clean. `--from` fails immediately; the provider gets one feedback round.
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
                let blockers = verifier_blockers(&parsed, &config);
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

    // `rivet plan` verifies through its own convergence loop (parse, the
    // Verifier, and a real compile) and carries no `--no-gauntlet` opt-out,
    // so it builds with the standalone Gauntlet step skipped.
    write_text(app_file, &source)?;
    if let Err(build_diagnostics) = run_build(app_file, BuildTarget::Native, true) {
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
        let blockers = verifier_blockers(&revised, &config);
        if !blockers.is_empty() {
            return Err(failed("compile", blockers));
        }
        write_text(app_file, &source)?;
        if let Err(build_diagnostics) = run_build(app_file, BuildTarget::Native, true) {
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

#[cfg(test)]
mod tests;
