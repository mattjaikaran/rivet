//! `rivet explain "<symptom>"`: trace a symptom to the route and commit
//! most likely to have introduced it (phase 2.2, pillar 04).
//!
//! The command re-indexes the current blueprint into the vector store —
//! one chunk per route, embedded deterministically — embeds the symptom,
//! and returns the nearest chunk. It then answers "which commit introduced
//! this route?" with git pickaxe: `git log -S <handler> -- <app>` finds the
//! commit that first added the matched handler's name to the module. The
//! module digest is recorded against the current commit as a fingerprint so
//! repeated runs can compare modules over time.
//!
//! Re-indexing on every run keeps the vector store in sync with the source
//! of truth on disk; it is idempotent.

use crate::config::RivetConfig;
use crate::diagnostic::Diagnostic;
use crate::gauntlet;
use crate::parser::python::parse_python_file;
use crate::store;
use crate::store::vector::{RouteSummary, digest, index_blueprint, search};
use std::path::Path;

/// Run `explain` inside a small tokio runtime so the async vector store
/// works from the otherwise synchronous command layer.
pub fn run_explain(symptom: &str, app_file: &Path) -> Result<(), Vec<Diagnostic>> {
    let project_dir = store::project_dir_for(app_file);
    let config = RivetConfig::load(&project_dir)
        .map_err(|message| vec![Diagnostic::blocker("E1008", message)])?;

    let module = parse_python_file(app_file)?;
    // Run the gauntlet too so the explanation carries the module's
    // findings; a module that fails the build is usually the module the
    // symptom points at.
    let findings = gauntlet::run_gauntlet(&module, &config.gauntlet);

    let routes: Vec<RouteSummary> = module
        .blueprint
        .routes
        .iter()
        .map(|route| RouteSummary {
            method: route.method.as_str().to_string(),
            path: route.path.clone(),
            handler: route.handler_name.clone(),
            stories: route.stories.clone(),
        })
        .collect();

    let module_text = format!("{:?}", module.blueprint);
    let module_digest = digest(&module_text);
    let app_label = app_file
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("app")
        .to_string();

    let commit = current_commit(&project_dir);
    let conn = store::open(&project_dir)?;
    if let Some(commit) = &commit {
        store::save_fingerprint(
            &conn,
            commit,
            app_file.to_string_lossy().as_ref(),
            &module_digest,
        )?;
    }

    // Index and search on the async runtime. Errors from the vector store
    // surface as a single structured diagnostic.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|err| {
            vec![Diagnostic::blocker(
                "E3005",
                format!("runtime error: {err}"),
            )]
        })?;
    let result = runtime.block_on(async {
        index_blueprint(&project_dir, &app_label, &routes)
            .await
            .map_err(|err| Diagnostic::blocker("E3005", err))?;
        search(&project_dir, &app_label, symptom)
            .await
            .map_err(|err| Diagnostic::blocker("E3005", err))
    })?;

    println!("Explain: {symptom:?}");
    match result.first() {
        Some((text, distance)) => {
            println!("Best matching route: {text}");
            println!("Distance: {distance:.4}");
            // Pick the matched route's handler out of the chunk text: the
            // chunk is "METHOD path handler NAME stories ...".
            let handler = text
                .split_whitespace()
                .skip_while(|w| *w != "handler")
                .nth(1);
            if let Some(handler) = handler {
                match introducing_commit(&project_dir, handler, app_file) {
                    Some(introducer) => {
                        println!("Introduced by commit: {introducer}");
                    }
                    None => {
                        println!(
                            "Could not find the commit that introduced handler {handler:?} \
                             (not a git checkout?)"
                        );
                    }
                }
            }
        }
        None => println!("No indexed routes match this symptom"),
    }
    match commit {
        Some(commit) => println!("Current commit {commit}, module digest {module_digest}"),
        None => println!("Not a git checkout; no commit fingerprint recorded"),
    }
    if findings.is_empty() {
        println!("Gauntlet: no findings on the module");
    } else {
        println!("Gauntlet: {} finding(s) on the module", findings.len());
    }
    Ok(())
}

/// Resolve the current git commit hash of the project, when it is a git
/// checkout. Returns `None` for a non-git directory.
fn current_commit(project_dir: &Path) -> Option<String> {
    git(project_dir, &["rev-parse", "HEAD"])
}

/// Find the commit that first added `needle` to `app_file`, via git
/// pickaxe (`git log -S`). Returns the introducing commit's hash.
fn introducing_commit(project_dir: &Path, needle: &str, app_file: &Path) -> Option<String> {
    let relative = app_file
        .strip_prefix(project_dir)
        .unwrap_or(app_file)
        .to_string_lossy();
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(project_dir)
        .args(["log", "--format=%H", "-S"])
        .arg(needle)
        .arg("--")
        .arg(relative.as_ref())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    // Newest commit first; the first line is the introducing commit.
    String::from_utf8(output.stdout)
        .ok()?
        .lines()
        .next()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
}

/// Run a git command and return its trimmed stdout on success.
fn git(project_dir: &Path, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(project_dir)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}
