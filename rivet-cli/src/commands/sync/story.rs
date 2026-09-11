//! The stories a blueprint declares, and the issue title each one expects.
//!
//! `rivet sync` binds a tracker issue to a story through the issue title:
//! the title starts with the story ID, then a colon, then the routes the
//! story covers — `US-001: GET /ping`. [`title_for`] builds that title, and
//! [`title_key`] reads the story ID back out of a title, so `--apply` writes
//! an issue the next run recognizes.

use crate::diagnostic::Diagnostic;
use rivet_core::ir::ServiceBlueprint;

/// One story the blueprint declares, with the title its issue carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Story {
    /// The story ID, as the decorator declared it, for example `US-001`.
    pub(super) id: String,
    /// The issue title: `<id>: <METHOD> <path>[; ...]`.
    pub(super) title: String,
}

/// Every story the blueprint declares, in first-use order, keyed by ID.
///
/// A story that several routes share covers all of them, so one issue tracks
/// the whole story. Returns [`E3023`](Diagnostic::blocker) when the blueprint
/// declares an ID the title format cannot carry.
pub(super) fn from_blueprint(
    blueprint: &ServiceBlueprint,
    app_file: &std::path::Path,
) -> Result<Vec<Story>, Diagnostic> {
    let mut ids: Vec<String> = Vec::new();
    for route in &blueprint.routes {
        for story in &route.stories {
            if !ids.contains(story) {
                ids.push(story.clone());
            }
        }
    }
    for id in &ids {
        if let Some(problem) = id_problem(id) {
            return Err(id_diagnostic(id, problem, app_file));
        }
    }
    Ok(ids
        .into_iter()
        .map(|id| {
            let title = title_for(&id, blueprint);
            Story { id, title }
        })
        .collect())
}

/// The issue title a story expects: the ID, then the routes it covers.
fn title_for(id: &str, blueprint: &ServiceBlueprint) -> String {
    let mut routes: Vec<String> = blueprint
        .routes
        .iter()
        .filter(|route| route.stories.iter().any(|story| story == id))
        .map(|route| format!("{} {}", route.method.as_str(), route.path))
        .collect();
    routes.sort();
    format!("{id}: {}", routes.join("; "))
}

/// The story ID an issue title names: the text before the first colon.
///
/// This is the exact binding: a story is tracked when this equals its ID, so
/// an ID the shape heuristic below would reject still round-trips through the
/// title `--apply` writes.
pub(super) fn title_key(title: &str) -> Option<&str> {
    let (key, _) = title.split_once(':')?;
    let key = key.trim();
    (!key.is_empty()).then_some(key)
}

/// The story ID an issue title names, when the title is shaped like one.
///
/// Only an ID-shaped prefix counts, so an unrelated tracker issue such as
/// `Fix the flaky test: again` never reports as an orphan. Tracking uses
/// [`title_key`] instead, so this narrow check never hides a declared story.
pub(super) fn key_of(title: &str) -> Option<&str> {
    let key = title_key(title)?;
    if key.len() > 64 {
        return None;
    }
    let shaped = key
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.');
    let has_letter = key.chars().any(|c| c.is_ascii_alphabetic());
    (shaped && has_letter).then_some(key)
}

/// The reason a story ID cannot go into an issue title.
fn id_problem(id: &str) -> Option<&'static str> {
    if id.trim() != id || id.is_empty() {
        return Some("it is empty or has leading or trailing whitespace");
    }
    if id.contains(':') {
        return Some("it holds a colon");
    }
    None
}

/// The `E3023` diagnostic for a story ID the title format cannot carry.
fn id_diagnostic(id: &str, problem: &str, app_file: &std::path::Path) -> Diagnostic {
    Diagnostic::blocker(
        "E3023",
        format!("`{id}` is not a usable story ID: {problem}"),
        "rename the story in the route's `stories=[...]` decorator, because the issue title binds to a story through the text before its first colon",
    )
    .located(app_file.display().to_string(), 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_core::ir::{Expr, HttpMethod, RequestSpec, ResponseSpec, RouteDefinition, TypeRef};
    use std::path::Path;

    fn route(method: HttpMethod, path: &str, stories: &[&str]) -> RouteDefinition {
        RouteDefinition {
            method,
            path: path.to_string(),
            handler_name: path.trim_start_matches('/').to_string(),
            stories: stories.iter().map(|story| story.to_string()).collect(),
            middlewares: vec![],
            request: RequestSpec::None,
            response: ResponseSpec::Json(TypeRef::Json),
            returns: vec![Expr::Null],
        }
    }

    fn blueprint(routes: Vec<RouteDefinition>) -> ServiceBlueprint {
        ServiceBlueprint {
            name: "app".to_string(),
            routes,
            structs: vec![],
            dependencies: vec![],
        }
    }

    #[test]
    fn a_story_is_listed_once_in_first_use_order() {
        let blueprint = blueprint(vec![
            route(HttpMethod::Get, "/ping", &["US-002", "US-001"]),
            route(HttpMethod::Post, "/echo", &["US-002"]),
        ]);
        let stories = from_blueprint(&blueprint, Path::new("app.py")).expect("the stories bind");
        assert_eq!(stories.len(), 2);
        assert_eq!(stories[0].id, "US-002");
        assert_eq!(stories[1].id, "US-001");
    }

    #[test]
    fn a_title_names_every_route_the_story_covers() {
        let blueprint = blueprint(vec![
            route(HttpMethod::Post, "/echo", &["US-002"]),
            route(HttpMethod::Get, "/ping", &["US-002"]),
        ]);
        let stories = from_blueprint(&blueprint, Path::new("app.py")).expect("the stories bind");
        assert_eq!(stories[0].title, "US-002: GET /ping; POST /echo");
    }

    #[test]
    fn a_generated_title_reads_back_as_its_own_story() {
        let blueprint = blueprint(vec![route(HttpMethod::Post, "/orders", &["123", "US.1"])]);
        let stories = from_blueprint(&blueprint, Path::new("app.py")).expect("the stories bind");
        for story in &stories {
            assert_eq!(
                title_key(&story.title),
                Some(story.id.as_str()),
                "every generated title round-trips: {story:?}"
            );
        }
    }

    #[test]
    fn a_story_id_the_title_cannot_carry_is_rejected() {
        let blueprint = blueprint(vec![route(HttpMethod::Get, "/ping", &["US:1"])]);
        let error = from_blueprint(&blueprint, Path::new("app.py"))
            .expect_err("a colon cannot go into a title");
        assert_eq!(error.error_code, "E3023");
        assert!(error.message.contains("US:1"));
        assert!(!error.suggested_fix.is_empty());
    }

    #[test]
    fn only_an_id_shaped_prefix_marks_an_orphan() {
        assert_eq!(key_of("US-001: GET /ping"), Some("US-001"));
        assert_eq!(key_of("US.1: GET /ping"), Some("US.1"));
        assert_eq!(
            key_of("ORD-7"),
            None,
            "an issue without a colon is no story"
        );
        assert_eq!(key_of("Fix the flaky test: again"), None, "not ID-shaped");
        assert_eq!(key_of(": no key"), None);
        assert_eq!(key_of("123: digits only"), None, "no letter");
        assert_eq!(title_key("123: digits only"), Some("123"));
    }
}
