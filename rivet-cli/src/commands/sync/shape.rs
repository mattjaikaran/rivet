//! The shape of a tracker payload: which provider produced it, and which
//! parser reads it.
//!
//! `--from` can run without tracker credentials, so the command reads the
//! shape out of the captured payload itself: a top-level `issues` array is a
//! Jira search answer, and a top-level `data.issues.nodes` array is a Linear
//! GraphQL answer.

use super::config::e3020;
use super::issue::Issue;
use crate::diagnostic::Diagnostic;
use serde_json::Value;

/// The tracker a payload belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Shape {
    /// A Jira REST API v3 answer.
    Jira,
    /// A Linear GraphQL answer.
    Linear,
}

impl Shape {
    /// The shape a captured payload has, from its top-level fields.
    pub(super) fn detect(payload: &str) -> Result<Shape, Diagnostic> {
        let answer: Value = serde_json::from_str(payload).map_err(|err| {
            e3020(
                format!("the captured tracker payload is not JSON: {err}"),
                "point --from at a tracker's JSON response, or set RIVET_JIRA_* or RIVET_LINEAR_* and drop --from",
            )
        })?;
        if answer.get("issues").and_then(Value::as_array).is_some() {
            return Ok(Shape::Jira);
        }
        if answer
            .get("data")
            .and_then(|data| data.get("issues"))
            .and_then(|issues| issues.get("nodes"))
            .and_then(Value::as_array)
            .is_some()
        {
            return Ok(Shape::Linear);
        }
        Err(e3020(
            "the captured tracker payload is neither a Jira search answer nor a Linear GraphQL answer",
            "point --from at a response with a top-level `issues` array (Jira) or a top-level `data.issues.nodes` array (Linear)",
        ))
    }

    /// Parse a payload of this shape into the issues the diff compares.
    pub(super) fn parse(self, payload: &str) -> Result<Vec<Issue>, Diagnostic> {
        match self {
            Shape::Jira => super::jira::parse_issues(payload),
            Shape::Linear => super::linear::parse_issues(payload),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_jira_answer_is_detected_from_its_issues_array() {
        let shape = Shape::detect(r#"{"issues":[{"key":"ORD-1","fields":{}}]}"#).expect("detect");
        assert_eq!(shape, Shape::Jira);
    }

    #[test]
    fn a_linear_answer_is_detected_from_its_graphql_shape() {
        let shape = Shape::detect(r#"{"data":{"issues":{"nodes":[]}}}"#).expect("detect");
        assert_eq!(shape, Shape::Linear);
    }

    #[test]
    fn another_json_shape_is_reported_with_a_fix() {
        let error = Shape::detect(r#"{"data":{"team":{}}}"#).expect_err("an unknown shape");
        assert_eq!(error.error_code, "E3020");
        assert!(!error.suggested_fix.is_empty());

        let error = Shape::detect("<html>").expect_err("a non-JSON body");
        assert_eq!(error.error_code, "E3020");
    }

    #[test]
    fn the_detected_shape_parses_its_own_payload() {
        let payload = r#"{"data":{"issues":{"nodes":[{"identifier":"ORD-1","title":"US-001: GET /ping","state":{"type":"started"}}]}}}"#;
        let issues = Shape::detect(payload)
            .expect("detect")
            .parse(payload)
            .expect("parse");
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].title, "US-001: GET /ping");
    }
}
