//! The Linear GraphQL provider for `rivet sync`.
//!
//! Linear serves one endpoint, `POST https://api.linear.app/graphql`, so both
//! requests are documents against it, built by pure functions:
//!
//! - [`issues_request`] — the team's issues with the title and state the diff
//!   needs.
//! - [`create_request`] — `issueCreate` for one missing story.
//!
//! [`parse_issues`] reads the answer. Linear reports an issue as closed when
//! its state type is `completed` or `canceled`.

use super::config::LinearConfig;
use super::config::e3020;
use super::http::Request;
use super::issue::Issue;
use crate::diagnostic::Diagnostic;
use serde_json::{Map, Value};

/// The Linear GraphQL endpoint.
pub(super) const ENDPOINT: &str = "https://api.linear.app/graphql";

/// How many issues one query page returns.
const PAGE_SIZE: u32 = 100;

/// The issues query: the team's issues, with title, state type, and key.
const ISSUES_QUERY: &str = "query($team: String!, $first: Int!) {\n  issues(filter: { team: { key: { eq: $team } } }, first: $first) {\n    nodes { identifier title state { type } }\n  }\n}";

/// The creation mutation: one issue, titled and parented to the team.
const CREATE_MUTATION: &str = "mutation($team: String!, $title: String!) {\n  issueCreate(input: { teamId: $team, title: $title }) {\n    success\n    issue { identifier }\n  }\n}";

/// The request that lists the team's issues.
pub(super) fn issues_request(cfg: &LinearConfig) -> Request {
    let mut variables = Map::new();
    variables.insert("team".into(), Value::String(cfg.team.clone()));
    variables.insert("first".into(), Value::from(PAGE_SIZE));
    graphql(cfg, ISSUES_QUERY, Value::Object(variables))
}

/// The request that creates one issue for a missing story.
pub(super) fn create_request(cfg: &LinearConfig, story: &super::story::Story) -> Request {
    let mut variables = Map::new();
    variables.insert("team".into(), Value::String(cfg.team.clone()));
    variables.insert("title".into(), Value::String(story.title.clone()));
    graphql(cfg, CREATE_MUTATION, Value::Object(variables))
}

/// One GraphQL document with its variables.
fn graphql(cfg: &LinearConfig, query: &str, variables: Value) -> Request {
    let mut body = Map::new();
    body.insert("query".into(), Value::String(query.to_string()));
    body.insert("variables".into(), variables);
    // A Linear personal API key travels raw in the `Authorization` header,
    // with no scheme in front of it.
    Request::post(ENDPOINT.to_string(), Value::Object(body).to_string()).authorization(&cfg.token)
}

/// Parse the issues answer into the issues the diff compares.
pub(super) fn parse_issues(payload: &str) -> Result<Vec<Issue>, Diagnostic> {
    let answer: Value = serde_json::from_str(payload).map_err(|err| {
        e3020(
            format!("the Linear answer is not JSON: {err}"),
            "check RIVET_LINEAR_TOKEN and RIVET_LINEAR_TEAM, then run rivet sync again; with --from, point it at a Linear GraphQL response",
        )
    })?;
    if let Some(errors) = answer.get("errors").and_then(Value::as_array)
        && !errors.is_empty()
    {
        return Err(e3020(
            format!("Linear reported an error: {}", Value::Array(errors.clone())),
            "check RIVET_LINEAR_TOKEN and RIVET_LINEAR_TEAM, then run rivet sync again",
        ));
    }
    let nodes = answer
        .get("data")
        .and_then(|data| data.get("issues"))
        .and_then(|issues| issues.get("nodes"))
        .and_then(Value::as_array)
        .ok_or_else(|| {
            e3020(
                "the Linear answer carries no `data.issues.nodes` array",
                "check RIVET_LINEAR_TEAM names a team key, then run rivet sync again; with --from, point it at a Linear GraphQL response",
            )
        })?;
    Ok(nodes
        .iter()
        .map(|node| Issue {
            key: text(node.get("identifier")),
            title: text(node.get("title")),
            closed: is_closed(node),
        })
        .filter(|issue| !issue.key.is_empty() && !issue.title.is_empty())
        .collect())
}

/// The key of the created issue in a mutation answer.
pub(super) fn parse_created(payload: &str) -> Result<String, Diagnostic> {
    let answer: Value = serde_json::from_str(payload).map_err(|err| {
        e3020(
            format!("the Linear answer to the create request is not JSON: {err}"),
            "run rivet sync --apply again; report the error if Linear keeps answering with a non-JSON body",
        )
    })?;
    let created = answer
        .get("data")
        .and_then(|data| data.get("issueCreate"))
        .ok_or_else(|| {
            e3020(
                format!("the Linear create answer carries no `data.issueCreate`: {payload}"),
                "check that RIVET_LINEAR_TOKEN may create issues in RIVET_LINEAR_TEAM, then run rivet sync --apply again",
            )
        })?;
    if created.get("success").and_then(Value::as_bool) != Some(true) {
        return Err(e3020(
            format!("Linear refused the issue: {payload}"),
            "check that RIVET_LINEAR_TOKEN may create issues in RIVET_LINEAR_TEAM, then run rivet sync --apply again",
        ));
    }
    Ok(text(
        created
            .get("issue")
            .and_then(|issue| issue.get("identifier")),
    ))
}

/// Whether the issue's state type says the work is finished.
fn is_closed(node: &Value) -> bool {
    matches!(
        text(node.get("state").and_then(|state| state.get("type"))).as_str(),
        "completed" | "canceled"
    )
}

/// A JSON string value, or an empty string.
fn text(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::sync::story::Story;

    /// A captured Linear GraphQL response.
    const CAPTURED: &str = r#"{
      "data": {
        "issues": {
          "nodes": [
            { "identifier": "ORD-1", "title": "US-001: GET /ping", "state": { "type": "started" } },
            { "identifier": "ORD-2", "title": "US-002: POST /echo", "state": { "type": "completed" } },
            { "identifier": "ORD-3", "title": "US-003: POST /orders", "state": { "type": "canceled" } }
          ]
        }
      }
    }"#;

    fn config() -> LinearConfig {
        LinearConfig {
            token: "lin_api_secret".to_string(),
            team: "ORD".to_string(),
        }
    }

    #[test]
    fn the_issues_request_is_one_document_with_the_team_key() {
        let request = issues_request(&config());
        assert_eq!(request.method, "POST");
        assert_eq!(request.url, ENDPOINT);
        assert_eq!(request.authorization.as_deref(), Some("lin_api_secret"));
        assert!(request.bearer.is_none(), "Linear takes the key raw");
        let body = request.body.expect("the query carries a body");
        assert!(body.contains("issues(filter"), "{body}");
        assert!(body.contains(r#""team":"ORD""#), "{body}");
        assert!(body.contains(r#""first":100"#), "{body}");
    }

    #[test]
    fn the_create_request_carries_the_title() {
        let story = Story {
            id: "US-004".to_string(),
            title: "US-004: GET /orders".to_string(),
        };
        let request = create_request(&config(), &story);
        let body = request.body.expect("the mutation carries a body");
        assert!(body.contains("issueCreate"), "{body}");
        assert!(body.contains(r#""title":"US-004: GET /orders""#), "{body}");
        assert!(body.contains(r#""team":"ORD""#), "{body}");
    }

    #[test]
    fn the_captured_answer_parses_into_issues() {
        let issues = parse_issues(CAPTURED).expect("the captured answer parses");
        assert_eq!(issues.len(), 3);
        assert_eq!(issues[0].key, "ORD-1");
        assert_eq!(issues[0].title, "US-001: GET /ping");
        assert!(!issues[0].closed, "a started issue is open");
        assert!(issues[1].closed, "a completed issue is closed");
        assert!(issues[2].closed, "a canceled issue is closed");
    }

    #[test]
    fn a_graphql_error_is_reported_with_a_fix() {
        let error = parse_issues(r#"{"errors":[{"message":"bad token"}]}"#)
            .expect_err("an errors array is an error");
        assert_eq!(error.error_code, "E3020");
        assert!(!error.suggested_fix.is_empty());
    }

    #[test]
    fn an_answer_without_the_issues_shape_is_reported() {
        let error = parse_issues(r#"{"data":{}}"#).expect_err("no issues is an error");
        assert_eq!(error.error_code, "E3020");
    }

    #[test]
    fn a_create_answer_names_the_new_issue() {
        let key = parse_created(
            r#"{"data":{"issueCreate":{"success":true,"issue":{"identifier":"ORD-4"}}}}"#,
        )
        .expect("the created key parses");
        assert_eq!(key, "ORD-4");

        let error = parse_created(r#"{"data":{"issueCreate":{"success":false}}}"#)
            .expect_err("a refused creation is an error");
        assert_eq!(error.error_code, "E3020");
    }
}
