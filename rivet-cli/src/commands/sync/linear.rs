//! The Linear GraphQL provider for `rivet sync`.
//!
//! Linear serves one endpoint, `POST https://api.linear.app/graphql`, so
//! every request is a document against it, built by pure functions:
//!
//! - [`issues_request`] — the team's issues with the title and state the diff
//!   needs, cursor-paged through `pageInfo`.
//! - [`team_request`] — the team's own ID. `issueCreate` takes a team ID, and
//!   `RIVET_LINEAR_TEAM` names a team key, so the creation path resolves one
//!   into the other once per run.
//! - [`create_request`] — `issueCreate` for one missing story.
//!
//! [`parse_page`] reads the answer. Linear reports an issue as closed when
//! its state type is `completed` or `canceled`.

use super::config::LinearConfig;
use super::config::e3020;
use super::http::{Page, Request};
use super::issue::Issue;
use super::story::Story;
use crate::diagnostic::Diagnostic;
use serde_json::{Map, Value};

/// The Linear GraphQL endpoint.
const ENDPOINT: &str = "https://api.linear.app/graphql";

/// How many issues one query page returns.
const PAGE_SIZE: u32 = 100;

/// The issues query: the team's issues, with title, state type, and key.
///
/// `pageInfo` carries the cursor, so the client reads every page instead of
/// treating a full first page as the whole tracker.
const ISSUES_QUERY: &str = "query($team: String!, $first: Int!, $after: String) {\n  issues(filter: { team: { key: { eq: $team } } }, first: $first, after: $after) {\n    nodes { identifier title state { type } }\n    pageInfo { hasNextPage endCursor }\n  }\n}";

/// The team query: the team's own ID for the creation mutation.
const TEAM_QUERY: &str = "query($team: String!) {\n  teams(filter: { key: { eq: $team } }, first: 1) {\n    nodes { id }\n  }\n}";

/// The creation mutation: one issue, titled and parented to the team.
const CREATE_MUTATION: &str = "mutation($team: String!, $title: String!) {\n  issueCreate(input: { teamId: $team, title: $title }) {\n    success\n    issue { identifier }\n  }\n}";

/// The request that lists the team's issues, from one page on.
pub(super) fn issues_request(cfg: &LinearConfig, after: Option<&str>) -> Request {
    let mut variables = Map::new();
    variables.insert("team".into(), Value::String(cfg.team.clone()));
    variables.insert("first".into(), Value::from(PAGE_SIZE));
    variables.insert(
        "after".into(),
        match after {
            Some(cursor) => Value::String(cursor.to_string()),
            None => Value::Null,
        },
    );
    graphql(cfg, ISSUES_QUERY, Value::Object(variables))
}

/// The request that resolves the team key into the team ID.
pub(super) fn team_request(cfg: &LinearConfig) -> Request {
    let mut variables = Map::new();
    variables.insert("team".into(), Value::String(cfg.team.clone()));
    graphql(cfg, TEAM_QUERY, Value::Object(variables))
}

/// The request that creates one issue for a missing story.
pub(super) fn create_request(cfg: &LinearConfig, team_id: &str, story: &Story) -> Request {
    let mut variables = Map::new();
    // `issueCreate` takes the team ID, not the key.
    variables.insert("team".into(), Value::String(team_id.to_string()));
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

/// Parse one issues page: its issues and the cursor that reads the next one.
pub(super) fn parse_page(payload: &str) -> Result<Page, Diagnostic> {
    let answer = answer(payload, "a Linear GraphQL response")?;
    let connection = answer
        .get("data")
        .and_then(|data| data.get("issues"))
        .ok_or_else(|| {
            e3020(
                "the Linear answer carries no `data.issues` connection",
                "check RIVET_LINEAR_TEAM names a team key, then run rivet sync again; with --from, point it at a Linear GraphQL response",
            )
        })?;
    let nodes = connection
        .get("nodes")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            e3020(
                "the Linear answer carries no `data.issues.nodes` array",
                "check RIVET_LINEAR_TEAM names a team key, then run rivet sync again; with --from, point it at a Linear GraphQL response",
            )
        })?;
    let issues = nodes
        .iter()
        .map(|node| Issue {
            key: text(node.get("identifier")),
            title: text(node.get("title")),
            closed: is_closed(node),
        })
        .filter(|issue| !issue.key.is_empty() && !issue.title.is_empty())
        .collect();

    let page_info = connection.get("pageInfo");
    let more = page_info
        .and_then(|info| info.get("hasNextPage"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let cursor = text(page_info.and_then(|info| info.get("endCursor")));
    Ok(Page {
        issues,
        next: (more && !cursor.is_empty()).then_some(cursor),
    })
}

/// The team ID from a team answer.
pub(super) fn parse_team(payload: &str) -> Result<String, Diagnostic> {
    let answer = answer(payload, "a Linear team query response")?;
    let id = answer
        .get("data")
        .and_then(|data| data.get("teams"))
        .and_then(|teams| teams.get("nodes"))
        .and_then(Value::as_array)
        .and_then(|nodes| nodes.first())
        .and_then(|team| team.get("id"));
    let id = text(id);
    if id.is_empty() {
        return Err(e3020(
            format!("Linear names no team for `RIVET_LINEAR_TEAM`: {payload}"),
            "check RIVET_LINEAR_TEAM is the team's key, then run rivet sync --apply again",
        ));
    }
    Ok(id)
}

/// The key of the created issue in a mutation answer.
pub(super) fn parse_created(payload: &str) -> Result<String, Diagnostic> {
    let answer = answer(payload, "a Linear create response")?;
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

/// Decode an answer and surface a GraphQL `errors` array.
fn answer(payload: &str, what: &str) -> Result<Value, Diagnostic> {
    let answer: Value = serde_json::from_str(payload).map_err(|err| {
        e3020(
            format!("{what} is not JSON: {err}"),
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
    Ok(answer)
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

    /// A captured Linear GraphQL response with a second page.
    const CAPTURED: &str = r#"{
      "data": {
        "issues": {
          "nodes": [
            { "identifier": "ORD-1", "title": "US-001: GET /ping", "state": { "type": "started" } },
            { "identifier": "ORD-2", "title": "US-002: POST /echo", "state": { "type": "completed" } },
            { "identifier": "ORD-3", "title": "US-003: POST /orders", "state": { "type": "canceled" } }
          ],
          "pageInfo": { "hasNextPage": true, "endCursor": "cursor-1" }
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
    fn the_issues_request_is_one_document_with_the_team_key_and_a_cursor() {
        let request = issues_request(&config(), Some("cursor-0"));
        assert_eq!(request.method, "POST");
        assert_eq!(request.url, ENDPOINT);
        assert_eq!(request.authorization.as_deref(), Some("lin_api_secret"));
        assert!(request.bearer.is_none(), "Linear takes the key raw");
        let body = request.body.expect("the query carries a body");
        assert!(body.contains("issues(filter"), "{body}");
        assert!(body.contains(r#""team":"ORD""#), "{body}");
        assert!(body.contains(r#""first":100"#), "{body}");
        assert!(body.contains(r#""after":"cursor-0""#), "{body}");
        assert!(
            body.contains("pageInfo"),
            "the query asks for the cursor: {body}"
        );
        // The first page sends an explicit null, which Linear reads as no
        // cursor.
        let first = issues_request(&config(), None);
        let body = first.body.expect("the query carries a body");
        assert!(body.contains(r#""after":null"#), "{body}");
    }

    #[test]
    fn the_team_request_resolves_the_key_to_an_id() {
        let request = team_request(&config());
        let body = request.body.expect("the query carries a body");
        assert!(body.contains("teams(filter"), "{body}");
        assert!(body.contains(r#""team":"ORD""#), "{body}");
    }

    #[test]
    fn the_create_request_carries_the_resolved_team_id() {
        let story = Story {
            id: "US-004".to_string(),
            title: "US-004: GET /orders".to_string(),
        };
        let request = create_request(&config(), "team-uuid-9", &story);
        let body = request.body.expect("the mutation carries a body");
        assert!(body.contains("issueCreate"), "{body}");
        assert!(body.contains(r#""title":"US-004: GET /orders""#), "{body}");
        assert!(
            body.contains(r#""team":"team-uuid-9""#),
            "the mutation takes the team ID: {body}"
        );
    }

    #[test]
    fn the_captured_page_parses_into_issues_and_a_cursor() {
        let page = parse_page(CAPTURED).expect("the captured answer parses");
        assert_eq!(page.issues.len(), 3);
        assert_eq!(page.issues[0].key, "ORD-1");
        assert_eq!(page.issues[0].title, "US-001: GET /ping");
        assert!(!page.issues[0].closed, "a started issue is open");
        assert!(page.issues[1].closed, "a completed issue is closed");
        assert!(page.issues[2].closed, "a canceled issue is closed");
        assert_eq!(page.next.as_deref(), Some("cursor-1"));
    }

    #[test]
    fn a_last_page_reports_no_cursor() {
        let payload = r#"{"data":{"issues":{"nodes":[],"pageInfo":{"hasNextPage":false,"endCursor":"cursor-9"}}}}"#;
        let page = parse_page(payload).expect("the page parses");
        assert!(page.next.is_none(), "hasNextPage false ends the search");

        let payload = r#"{"data":{"issues":{"nodes":[]}}}"#;
        let page = parse_page(payload).expect("a page without pageInfo parses");
        assert!(page.next.is_none());
    }

    #[test]
    fn a_team_answer_names_the_team_id() {
        let id = parse_team(r#"{"data":{"teams":{"nodes":[{"id":"team-uuid-9"}]}}}"#)
            .expect("the team id parses");
        assert_eq!(id, "team-uuid-9");

        let error =
            parse_team(r#"{"data":{"teams":{"nodes":[]}}}"#).expect_err("no team is an error");
        assert_eq!(error.error_code, "E3020");
        assert!(!error.suggested_fix.is_empty());
    }

    #[test]
    fn a_graphql_error_is_reported_with_a_fix() {
        let error = parse_page(r#"{"errors":[{"message":"bad token"}]}"#)
            .expect_err("an errors array is an error");
        assert_eq!(error.error_code, "E3020");
        assert!(!error.suggested_fix.is_empty());
    }

    #[test]
    fn an_answer_without_the_issues_shape_is_reported() {
        let error = parse_page(r#"{"data":{}}"#).expect_err("no issues is an error");
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
