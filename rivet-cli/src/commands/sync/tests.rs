//! End-to-end `rivet sync` tests: a fixture app, a captured tracker payload,
//! and the diff the command reports for it.
//!
//! The provider is passed in, so these tests never touch the process
//! environment and never reach the network.

use super::*;
use crate::commands::sync::config::{JiraConfig, LinearConfig};
use crate::test_support::ScratchDir;
use std::fs;
use std::path::PathBuf;

/// A fixture app with three stories: one route carries two of them.
const FIXTURE_APP: &str = "from rivet import api\n\n@api.get(\"/ping\", stories=[\"US-001\"])\ndef ping() -> dict:\n    return {\"status\": \"pong\"}\n\n@api.post(\"/echo\", stories=[\"US-002\"])\ndef echo(request: dict) -> dict:\n    return {\"echo\": request}\n\n@api.post(\"/orders\", stories=[\"US-002\", \"US-003\"])\ndef create_order(request: dict) -> dict:\n    return {\"order\": request}\n";

/// A captured Jira search response: US-001 agrees, US-002 is closed, and
/// ORD-9 names a story the fixture does not declare.
const CAPTURED_JIRA: &str = r#"{
  "issues": [
    { "key": "ORD-1", "fields": { "summary": "US-001: GET /ping", "status": { "statusCategory": { "key": "indeterminate" } } } },
    { "key": "ORD-2", "fields": { "summary": "US-002: POST /echo; POST /orders", "status": { "statusCategory": { "key": "done" } } } },
    { "key": "ORD-9", "fields": { "summary": "US-999: POST /gone", "status": { "statusCategory": { "key": "new" } } } }
  ]
}"#;

/// A captured Jira answer that already covers every story.
const CAPTURED_JIRA_AGREEING: &str = r#"{
  "issues": [
    { "key": "ORD-1", "fields": { "summary": "US-001: GET /ping", "status": { "statusCategory": { "key": "indeterminate" } } } },
    { "key": "ORD-2", "fields": { "summary": "US-002: POST /echo; POST /orders", "status": { "statusCategory": { "key": "started" } } } },
    { "key": "ORD-3", "fields": { "summary": "US-003: POST /orders", "status": { "statusCategory": { "key": "new" } } } }
  ]
}"#;

/// A captured Linear GraphQL answer, shaped as the live query returns it.
const CAPTURED_LINEAR: &str = r#"{
  "data": { "issues": { "nodes": [
    { "identifier": "ORD-1", "title": "US-001: GET /ping", "state": { "type": "started" } },
    { "identifier": "ORD-2", "title": "US-002: POST /echo; POST /orders", "state": { "type": "completed" } }
  ] } }
}"#;

/// A scratch project with the fixture app and one captured payload file.
fn project(name: &str, captured: &str) -> (ScratchDir, PathBuf, PathBuf) {
    let dir = ScratchDir::new(name);
    let app = dir.join("app.py");
    fs::write(&app, FIXTURE_APP).expect("write app.py");
    let payload = dir.join("payload.json");
    fs::write(&payload, captured).expect("write the captured payload");
    (dir, app, payload)
}

fn jira_provider() -> Provider {
    Provider::Jira(JiraConfig {
        base_url: "https://example.atlassian.net".to_string(),
        token: "token".to_string(),
        project: "ORD".to_string(),
    })
}

fn linear_provider() -> Provider {
    Provider::Linear(LinearConfig {
        token: "token".to_string(),
        team: "ORD".to_string(),
    })
}

/// A configured tracker, as `--from` uses it: the payload decides nothing.
fn configured() -> Result<Provider, Diagnostic> {
    Ok(jira_provider())
}

/// A configured Linear tracker, so the provider decides the payload shape.
fn configured_linear() -> Result<Provider, Diagnostic> {
    Ok(linear_provider())
}

/// No tracker configured, as an offline CI job runs it.
fn unconfigured() -> Result<Provider, Diagnostic> {
    Err(config::e3019("no tracker is configured", "set a tracker"))
}

/// Run the command against a captured payload, resolving the source exactly
/// as `run_sync` does.
fn run_captured(
    configured: Result<Provider, Diagnostic>,
    app: &Path,
    payload: &Path,
) -> Result<(Outcome, Diff), Diagnostic> {
    let source = source(configured, Some(payload))?;
    run_with(source, app, false)
}

#[test]
fn a_captured_payload_reports_the_expected_diff() {
    let (_dir, app, payload) = project("sync-diff", CAPTURED_JIRA);
    // An offline run: no credentials, so the payload names its own shape.
    let (outcome, diff) = run_captured(unconfigured(), &app, &payload).expect("the diff computes");

    assert_eq!(outcome, Outcome::Diverged);
    assert_eq!(diff.missing.len(), 1);
    assert_eq!(diff.missing[0].id, "US-003");
    assert_eq!(diff.missing[0].title, "US-003: POST /orders");
    assert_eq!(diff.orphan.len(), 1);
    assert_eq!(diff.orphan[0].key, "ORD-9");
    assert_eq!(diff.drifted.len(), 1, "a closed issue drifts: {diff:?}");
    assert_eq!(diff.drifted[0].kind, issue::DriftKind::State);
    assert_eq!(diff.summary(), "1 missing, 1 orphan, 1 drifted");

    let lines = diff.lines();
    assert!(lines[0].starts_with("missing issue: US-003"), "{lines:?}");
    assert!(lines[1].starts_with("orphan issue: ORD-9"), "{lines:?}");
    assert!(lines[2].starts_with("state drift: ORD-2"), "{lines:?}");
}

#[test]
fn a_payload_that_covers_every_story_reports_an_empty_diff() {
    let (_dir, app, payload) = project("sync-agreed", CAPTURED_JIRA_AGREEING);
    let (outcome, diff) = run_captured(configured(), &app, &payload).expect("the diff computes");

    assert_eq!(outcome, Outcome::Agreed, "{diff:?}");
    assert!(diff.is_empty(), "{diff:?}");
    assert_eq!(diff.summary(), "the blueprint and the tracker agree");
}

#[test]
fn a_linear_payload_drives_the_same_diff() {
    let (_dir, app, payload) = project("sync-linear", CAPTURED_LINEAR);
    let (outcome, diff) =
        run_captured(configured_linear(), &app, &payload).expect("the diff computes");

    assert_eq!(outcome, Outcome::Diverged);
    assert_eq!(diff.missing.len(), 1);
    assert_eq!(diff.missing[0].id, "US-003");
    assert_eq!(diff.drifted.len(), 1);
    assert_eq!(diff.drifted[0].kind, issue::DriftKind::State);
    assert!(diff.orphan.is_empty());
}

#[test]
fn a_missing_captured_payload_is_reported_with_a_fix() {
    let (_dir, app, _payload) = project("sync-no-payload", CAPTURED_JIRA);
    let missing = app.with_file_name("absent.json");
    let error = run_captured(configured(), &app, &missing).expect_err("the file is absent");
    assert_eq!(error.error_code, "E3020");
    assert!(!error.suggested_fix.is_empty());
}

#[test]
fn a_payload_the_provider_cannot_read_is_reported_with_a_fix() {
    let (_dir, app, payload) = project("sync-bad-payload", "{\"unexpected\":true}");
    let error = run_captured(configured(), &app, &payload).expect_err("the shape is wrong");
    assert_eq!(error.error_code, "E3020");
    assert!(!error.suggested_fix.is_empty());
}
