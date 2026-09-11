//! `rivet sync [app.py] [--dry-run] [--apply] [--from FILE]`: reconcile the
//! blueprint's story IDs with Jira or Linear (phase 4, pillar 05).
//!
//! The command reads the stories the blueprint declares, lists the issues the
//! tracker holds, and reports the diff: a story with no issue, an issue with
//! no story, a title that no longer matches the routes the story covers, and
//! an issue the tracker closed while the blueprint still serves it.
//!
//! Nothing is written unless `--apply` is passed, and `--apply` only creates
//! the missing issues: the command never edits or deletes a tracker issue a
//! human owns.
//!
//! `--from FILE` reads a captured tracker payload instead of calling the
//! tracker, so a test and a CI job compute the same diff offline. With no
//! credentials configured, the command reads the payload's shape to pick the
//! parser, so the offline path needs no secrets at all.
//!
//! Codes: E3019 tracker configuration, E3020 tracker request or answer,
//! E3021 tracker write, E3022 the diff the run could not reconcile, E3023 a
//! story ID the issue title cannot carry.

use crate::diagnostic::Diagnostic;
use crate::parser::python::parse_python_file;
use std::future::Future;
use std::path::Path;

mod config;
mod http;
mod issue;
mod jira;
mod linear;
mod shape;
mod story;
#[cfg(test)]
mod tests;

use config::{Provider, read_payload};
use http::Page;
use issue::{Diff, Issue};
use shape::Shape;
use story::Story;

/// The most pages one tracker read may take.
///
/// The cap bounds a tracker whose paging never converges: a repeated token
/// and an alternating cycle both stop here, instead of growing the issue
/// list without bound.
const MAX_PAGES: usize = 64;

/// What the diff means for the command's exit code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    /// The blueprint and the tracker agree.
    Agreed,
    /// Differences remain, so the command fails for a CI job to act on.
    Diverged,
}

/// Where the issues come from, and where new ones go.
enum Source {
    /// A configured tracker: read it, and create on `--apply`.
    Tracker(Provider),
    /// A captured payload: read the payload, and write nothing.
    Captured { shape: Shape, payload: String },
}

/// Run `rivet sync`.
///
/// A reported difference is not a crash: the diff prints either way, and a
/// difference that survives the run fails the command, so a pipeline can gate
/// on it.
pub fn run_sync(app_file: &Path, from: Option<&Path>, apply: bool) -> Result<(), Vec<Diagnostic>> {
    let source = source(config::from_env(), from).map_err(|diagnostic| vec![diagnostic])?;
    match run_with(source, app_file, apply) {
        Ok((Outcome::Agreed, _)) => Ok(()),
        Ok((Outcome::Diverged, _)) => Err(vec![diverged(app_file)]),
        Err(diagnostic) => Err(vec![diagnostic]),
    }
}

/// Resolve what the run reads.
///
/// A configured tracker wins; a captured payload decides the parser from its
/// own shape when no tracker is configured; with neither, the configuration
/// error stands.
fn source(
    configured: Result<Provider, Diagnostic>,
    from: Option<&Path>,
) -> Result<Source, Diagnostic> {
    let payload = match from {
        Some(path) => Some(read_payload(path)?),
        None => None,
    };
    match (configured, payload) {
        (Ok(provider), None) => Ok(Source::Tracker(provider)),
        (Ok(provider), Some(payload)) => Ok(Source::Captured {
            shape: provider.shape(),
            payload,
        }),
        (Err(_), Some(payload)) => Ok(Source::Captured {
            shape: Shape::detect(&payload)?,
            payload,
        }),
        (Err(diagnostic), None) => Err(diagnostic),
    }
}

/// Reconcile one app against one source, print the diff, and decide the
/// outcome.
fn run_with(source: Source, app_file: &Path, apply: bool) -> Result<(Outcome, Diff), Diagnostic> {
    let module = parse_python_file(app_file)?;
    let stories = story::from_blueprint(&module.blueprint, app_file)?;

    let (issues, tracker) = match &source {
        Source::Tracker(provider) => (read_tracker(provider)?, Some(provider)),
        Source::Captured { shape, payload } => (shape.parse(payload)?.issues, None),
    };
    let mut diff = issue::compute(&stories, &issues);

    println!(
        "rivet sync: {} stories in the blueprint, {} issues in the tracker",
        stories.len(),
        issues.len()
    );
    for line in diff.lines() {
        println!("{line}");
    }
    if apply && !diff.missing.is_empty() {
        let tracker = tracker.ok_or_else(|| {
            config::e3019(
                "a captured payload cannot be written to the tracker",
                "drop --from and configure RIVET_JIRA_* or RIVET_LINEAR_*, then run rivet sync --apply again",
            )
        })?;
        create_missing(tracker, &diff.missing)?;
        // Every created issue carries exactly the derived title, so the
        // stories it covers are no longer missing.
        diff.created();
    }
    println!("diff: {}", diff.summary());

    let outcome = if diff.is_empty() {
        Outcome::Agreed
    } else {
        Outcome::Diverged
    };
    Ok((outcome, diff))
}

/// Read every page of the tracker's issues.
///
/// A tracker with more issues than one page must be read to the end: a short
/// read would report the later stories as missing and let `--apply` file
/// duplicate issues on every run.
fn read_tracker(provider: &Provider) -> Result<Vec<Issue>, Diagnostic> {
    let mut issues = Vec::new();
    let mut page_token: Option<String> = None;
    // A tracker that never stops handing out tokens must not page forever:
    // the cap bounds every cycle, not just a repeated token.
    for _ in 0..MAX_PAGES {
        let request = match provider {
            Provider::Jira(cfg) => jira::issues_request(cfg, page_token.as_deref()),
            Provider::Linear(cfg) => linear::issues_request(cfg, page_token.as_deref()),
        };
        let body = read(request)?;
        let page: Page = match provider {
            Provider::Jira(_) => jira::parse_page(&body)?,
            Provider::Linear(_) => linear::parse_page(&body)?,
        };
        let next = page.next;
        issues.extend(page.issues);
        // A page that names the token just used would loop; fail on it at
        // once instead of paying for the whole page cap.
        if next.is_some() && next == page_token {
            return Err(config::e3020(
                format!(
                    "the tracker repeated its page token {:?}, so paging cannot finish",
                    next.as_deref().unwrap_or_default()
                ),
                "report this error: the tracker's paging is not converging, so rerun rivet sync later or reconcile the issues by hand",
            ));
        }
        match next {
            Some(token) => page_token = Some(token),
            None => return Ok(issues),
        }
    }
    Err(config::e3020(
        format!("the tracker is still paging after {MAX_PAGES} pages, so the read cannot finish"),
        "report this error: the tracker's paging is not converging, so rerun rivet sync later or reconcile the issues by hand",
    ))
}

/// Send a read request; a failure is an `E3020` diagnostic.
fn read(request: http::Request) -> Result<String, Diagnostic> {
    block_on(http::send(&request)).map_err(|detail| {
        config::e3020(
            detail,
            "check the RIVET_JIRA_* or RIVET_LINEAR_* variables and that the tracker is reachable, then run rivet sync again",
        )
    })
}

/// Send a write request; a failure is an `E3021` diagnostic.
fn write(request: http::Request) -> Result<String, Diagnostic> {
    block_on(http::send(&request)).map_err(|detail| {
        config::e3021(
            detail,
            "check that the token may create issues in the configured project or team, then run rivet sync --apply again",
        )
    })
}

/// Create one issue per missing story, and report the key of each.
fn create_missing(provider: &Provider, missing: &[Story]) -> Result<(), Diagnostic> {
    // Linear's creation takes a team ID, and the configuration names a team
    // key, so resolve one into the other once.
    let linear_team = match provider {
        Provider::Jira(_) => None,
        Provider::Linear(cfg) => Some(linear::parse_team(&write(linear::team_request(cfg))?)?),
    };
    for story in missing {
        let request = match provider {
            Provider::Jira(cfg) => jira::create_request(cfg, story),
            Provider::Linear(cfg) => {
                let team = linear_team.as_deref().unwrap_or_default();
                linear::create_request(cfg, team, story)
            }
        };
        let answer = write(request)?;
        let key = match provider {
            Provider::Jira(_) => jira::parse_created(&answer),
            Provider::Linear(_) => linear::parse_created(&answer),
        }?;
        println!("created {key} ({})", story.title);
    }
    Ok(())
}

/// Run one tracker future to completion on a current-thread runtime.
///
/// A failure is a plain message: the caller knows whether the request it
/// sent was a read or a write, and picks the code accordingly.
fn block_on<T>(future: impl Future<Output = Result<T, String>>) -> Result<T, String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|err| format!("failed to start the async runtime: {err}"))?;
    runtime.block_on(future)
}

/// The diagnostic that reports a diff the run could not reconcile.
fn diverged(app_file: &Path) -> Diagnostic {
    config::e3022(
        "the blueprint and the tracker still disagree",
        "run `rivet sync --apply` to create the missing issues, then reconcile the drift and the orphan issues in the tracker",
    )
    .located(app_file.display().to_string(), 1)
}
