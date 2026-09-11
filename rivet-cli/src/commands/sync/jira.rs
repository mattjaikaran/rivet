//! The Jira REST API v3 provider for `rivet sync`.
//!
//! Two requests, both built by pure functions so a unit test pins the shape
//! without a network:
//!
//! - [`issues_request`] — `GET /rest/api/3/search` over the configured
//!   project, asking for the fields the diff needs.
//! - [`create_request`] — `POST /rest/api/3/issue` for one missing story.
//!
//! [`parse_issues`] reads the search answer. Jira reports an issue as closed
//! when its status category is `done`, which is stable across the status
//! names a project chooses.

use super::config::JiraConfig;
use super::config::e3020;
use super::http::Request;
use super::issue::Issue;
use crate::diagnostic::Diagnostic;
use serde_json::{Map, Value};

/// The fields the search asks for.
const FIELDS: &str = "summary,status,labels";

/// How many issues one search page returns.
const PAGE_SIZE: u32 = 100;

/// The issue type a created issue uses.
const ISSUE_TYPE: &str = "Task";

/// The request that lists the project's issues.
pub(super) fn issues_request(cfg: &JiraConfig) -> Request {
    let jql = format!("project = \"{}\" ORDER BY key ASC", cfg.project);
    let url = format!(
        "{}/rest/api/3/search?jql={}&maxResults={PAGE_SIZE}&fields={FIELDS}",
        cfg.base_url.trim_end_matches('/'),
        encode_query(&jql),
    );
    Request::get(url).bearer(&cfg.token)
}

/// The request that creates one issue for a missing story.
pub(super) fn create_request(cfg: &JiraConfig, story: &super::story::Story) -> Request {
    let mut fields = Map::new();
    fields.insert("summary".into(), Value::String(story.title.clone()));
    let mut project = Map::new();
    project.insert("key".into(), Value::String(cfg.project.clone()));
    fields.insert("project".into(), Value::Object(project));
    let mut issue_type = Map::new();
    issue_type.insert("name".into(), Value::String(ISSUE_TYPE.to_string()));
    fields.insert("issuetype".into(), Value::Object(issue_type));
    let mut body = Map::new();
    body.insert("fields".into(), Value::Object(fields));

    Request::post(
        format!("{}/rest/api/3/issue", cfg.base_url.trim_end_matches('/')),
        Value::Object(body).to_string(),
    )
    .bearer(&cfg.token)
}

/// Parse the search answer into the issues the diff compares.
pub(super) fn parse_issues(payload: &str) -> Result<Vec<Issue>, Diagnostic> {
    let answer: Value = serde_json::from_str(payload).map_err(|err| {
        e3020(
            format!("the Jira answer is not JSON: {err}"),
            "check RIVET_JIRA_BASE_URL and RIVET_JIRA_TOKEN, then run rivet sync again; with --from, point it at a Jira search response",
        )
    })?;
    let issues = answer.get("issues").and_then(Value::as_array).ok_or_else(|| {
        e3020(
            "the Jira answer carries no `issues` array",
            "check RIVET_JIRA_BASE_URL and RIVET_JIRA_PROJECT, then run rivet sync again; with --from, point it at a Jira search response",
        )
    })?;
    Ok(issues
        .iter()
        .map(|issue| {
            let fields = issue.get("fields").unwrap_or(&Value::Null);
            Issue {
                key: text(issue.get("key")),
                title: text(fields.get("summary")),
                closed: is_closed(fields),
            }
        })
        .filter(|issue| !issue.key.is_empty())
        .collect())
}

/// The key of the created issue in a create answer.
pub(super) fn parse_created(payload: &str) -> Result<String, Diagnostic> {
    let answer: Value = serde_json::from_str(payload).map_err(|err| {
        e3020(
            format!("the Jira answer to the create request is not JSON: {err}"),
            "run rivet sync --apply again; report the error if the Jira site keeps answering with a non-JSON body",
        )
    })?;
    let key = text(answer.get("key"));
    if key.is_empty() {
        return Err(e3020(
            format!("the Jira create answer names no issue key: {payload}"),
            "check that RIVET_JIRA_TOKEN may create issues in RIVET_JIRA_PROJECT, then run rivet sync --apply again",
        ));
    }
    Ok(key)
}

/// Whether the issue's status category says the work is done.
fn is_closed(fields: &Value) -> bool {
    let category = fields
        .get("status")
        .and_then(|status| status.get("statusCategory"))
        .and_then(|category| category.get("key"));
    text(category) == "done"
}

/// A JSON string value, or an empty string.
fn text(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// Percent-encode a JQL query: everything outside the unreserved set.
fn encode_query(query: &str) -> String {
    let mut out = String::with_capacity(query.len());
    for byte in query.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::sync::story::Story;

    /// A captured Jira search response.
    const CAPTURED: &str = r#"{
      "startAt": 0,
      "maxResults": 100,
      "total": 3,
      "issues": [
        {
          "key": "ORD-1",
          "fields": {
            "summary": "US-001: GET /ping",
            "labels": ["rivet"],
            "status": { "name": "In Progress", "statusCategory": { "key": "indeterminate" } }
          }
        },
        {
          "id": "10002",
          "key": "ORD-2",
          "fields": {
            "summary": "US-002: POST /echo",
            "status": { "name": "Done", "statusCategory": { "key": "done" } }
          }
        },
        {
          "id": "10003",
          "key": "ORD-3",
          "fields": {
            "summary": "Update the docs",
            "status": { "name": "To Do", "statusCategory": { "key": "new" } }
          }
        }
      ]
    }"#;

    fn config() -> JiraConfig {
        JiraConfig {
            base_url: "https://example.atlassian.net/".to_string(),
            token: "secret".to_string(),
            project: "ORD".to_string(),
        }
    }

    #[test]
    fn the_search_request_names_the_project_and_asks_for_the_fields() {
        let request = issues_request(&config());
        assert_eq!(request.method, "GET");
        assert!(
            request
                .url
                .starts_with("https://example.atlassian.net/rest/api/3/search?"),
            "{}",
            request.url
        );
        assert!(
            request.url.contains("jql=project%20%3D%20%22ORD%22"),
            "the JQL is percent-encoded: {}",
            request.url
        );
        assert!(request.url.contains("fields=summary,status,labels"));
        assert_eq!(request.bearer.as_deref(), Some("secret"));
        assert!(request.body.is_none());
    }

    #[test]
    fn the_create_request_carries_the_title_and_the_project() {
        let story = Story {
            id: "US-003".to_string(),
            title: "US-003: GET /orders".to_string(),
        };
        let request = create_request(&config(), &story);
        assert_eq!(request.method, "POST");
        assert_eq!(
            request.url,
            "https://example.atlassian.net/rest/api/3/issue"
        );
        assert_eq!(request.bearer.as_deref(), Some("secret"));
        let body = request.body.expect("the create request carries a body");
        assert!(
            body.contains(r#""summary":"US-003: GET /orders""#),
            "{body}"
        );
        assert!(body.contains(r#""key":"ORD""#), "{body}");
        assert!(body.contains(r#""name":"Task""#), "{body}");
    }

    #[test]
    fn the_captured_answer_parses_into_issues() {
        let issues = parse_issues(CAPTURED).expect("the captured answer parses");
        assert_eq!(issues.len(), 3);
        assert_eq!(issues[0].key, "ORD-1");
        assert_eq!(issues[0].title, "US-001: GET /ping");
        assert!(!issues[0].closed, "an indeterminate category is open");
        assert!(issues[1].closed, "a done category is closed");
        assert!(!issues[2].closed);
    }

    #[test]
    fn an_answer_without_issues_is_reported_with_a_fix() {
        let error = parse_issues(r#"{"errorMessages":["bad token"]}"#)
            .expect_err("a missing issues array is an error");
        assert_eq!(error.error_code, "E3020");
        assert!(!error.suggested_fix.is_empty());

        let error = parse_issues("not json").expect_err("a non-JSON body is an error");
        assert_eq!(error.error_code, "E3020");
    }

    #[test]
    fn a_create_answer_names_the_new_issue() {
        let key = parse_created(r#"{"id":"10004","key":"ORD-4","self":"https://x"}"#)
            .expect("the created key parses");
        assert_eq!(key, "ORD-4");
        let error = parse_created(r#"{"errors":{}}"#).expect_err("no key is an error");
        assert_eq!(error.error_code, "E3020");
    }
}
