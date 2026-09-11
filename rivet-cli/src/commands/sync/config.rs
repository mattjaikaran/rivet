//! Tracker configuration for `rivet sync`, read from the environment.
//!
//! Credentials never live in the repository, so the tracker a project
//! reconciles with comes from the environment alone:
//!
//! - Jira: `RIVET_JIRA_BASE_URL`, `RIVET_JIRA_TOKEN`, `RIVET_JIRA_PROJECT`
//! - Linear: `RIVET_LINEAR_TOKEN`, `RIVET_LINEAR_TEAM`
//!
//! Exactly one provider must be configured. Neither, or both, is an `E3019`
//! diagnostic that names the variables the user has to set or unset.

use super::shape::Shape;
use crate::diagnostic::Diagnostic;
use std::path::Path;

/// The tracker a project reconciles with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Provider {
    /// The Jira REST API v3.
    Jira(JiraConfig),
    /// The Linear GraphQL API.
    Linear(LinearConfig),
}

/// The Jira settings, resolved from the environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct JiraConfig {
    /// The site root, for example `https://example.atlassian.net`.
    pub(super) base_url: String,
    /// The personal access token, sent as a bearer token.
    pub(super) token: String,
    /// The project key the issues belong to, for example `ORD`.
    pub(super) project: String,
}

/// The Linear settings, resolved from the environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LinearConfig {
    /// The personal API key, sent raw in the `Authorization` header.
    pub(super) token: String,
    /// The team key the issues belong to, for example `ORD`.
    pub(super) team: String,
}

/// Read the tracker configuration from the environment.
///
/// A project configures one provider. Zero or two configured providers is an
/// error, because the command cannot guess which tracker owns the stories.
pub(super) fn from_env() -> Result<Provider, Diagnostic> {
    let jira = read_jira();
    let linear = read_linear();
    match (jira, linear) {
        (Some(jira), None) => Ok(Provider::Jira(jira)),
        (None, Some(linear)) => Ok(Provider::Linear(linear)),
        (None, None) => Err(e3019(
            "no tracker is configured",
            "export RIVET_JIRA_BASE_URL, RIVET_JIRA_TOKEN, and RIVET_JIRA_PROJECT for Jira, or RIVET_LINEAR_TOKEN and RIVET_LINEAR_TEAM for Linear, then run rivet sync again",
        )),
        (Some(_), Some(_)) => Err(e3019(
            "both Jira and Linear are configured",
            "unset the RIVET_JIRA_* or the RIVET_LINEAR_* variables so exactly one tracker is configured, then run rivet sync again",
        )),
    }
}

impl Provider {
    /// The payload shape this provider reads and writes.
    pub(super) fn shape(&self) -> Shape {
        match self {
            Provider::Jira(_) => Shape::Jira,
            Provider::Linear(_) => Shape::Linear,
        }
    }
}

/// The Jira configuration, when the environment names a complete one.
fn read_jira() -> Option<JiraConfig> {
    Some(JiraConfig {
        base_url: var("RIVET_JIRA_BASE_URL")?,
        token: var("RIVET_JIRA_TOKEN")?,
        project: var("RIVET_JIRA_PROJECT")?,
    })
}

/// The Linear configuration, when the environment names a complete one.
fn read_linear() -> Option<LinearConfig> {
    Some(LinearConfig {
        token: var("RIVET_LINEAR_TOKEN")?,
        team: var("RIVET_LINEAR_TEAM")?,
    })
}

/// A non-empty environment variable.
fn var(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.is_empty())
}

/// Read a captured tracker payload from a file.
///
/// `--from` makes the command deterministic and offline: it reads the
/// tracker's answer from disk instead of the network, so a test and a CI job
/// exercise the same diff the live command computes.
pub(super) fn read_payload(path: &Path) -> Result<String, Diagnostic> {
    std::fs::read_to_string(path).map_err(|err| {
        e3020(
            format!("cannot read the captured tracker payload {}: {err}", path.display()),
            "point --from at a file that holds a tracker JSON response, or drop --from to call the tracker",
        )
    })
}

/// An `E3019` diagnostic: the tracker configuration is unusable.
pub(super) fn e3019(message: impl Into<String>, fix: impl Into<String>) -> Diagnostic {
    Diagnostic::blocker("E3019", message, fix)
}

/// An `E3020` diagnostic: the tracker request or its answer failed.
pub(super) fn e3020(message: impl Into<String>, fix: impl Into<String>) -> Diagnostic {
    Diagnostic::blocker("E3020", message, fix)
}

/// An `E3021` diagnostic: writing to the tracker failed.
pub(super) fn e3021(message: impl Into<String>, fix: impl Into<String>) -> Diagnostic {
    Diagnostic::blocker("E3021", message, fix)
}

/// An `E3022` diagnostic: the diff survived the run.
pub(super) fn e3022(message: impl Into<String>, fix: impl Into<String>) -> Diagnostic {
    Diagnostic::blocker("E3022", message, fix)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_payload_that_does_not_exist_reports_a_fix() {
        let error = read_payload(Path::new("/nonexistent/payload.json"))
            .expect_err("a missing file is an error");
        assert_eq!(error.error_code, "E3020");
        assert!(!error.suggested_fix.is_empty());
    }
}
