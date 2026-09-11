//! The stories a blueprint declares, and the issue title each one expects.
//!
//! `rivet sync` binds a tracker issue to a story through the issue title:
//! the title starts with the story ID, then a colon, then the routes the
//! story covers — `US-001: GET /ping`. [`title_for`] builds that title, and
//! [`key_of`] reads the story ID back out of a title, so `--apply` writes an
//! issue the next run recognizes.

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
/// the whole story.
pub(super) fn from_blueprint(blueprint: &ServiceBlueprint) -> Vec<Story> {
    let mut ids: Vec<String> = Vec::new();
    for route in &blueprint.routes {
        for story in &route.stories {
            if !ids.contains(story) {
                ids.push(story.clone());
            }
        }
    }
    ids.into_iter()
        .map(|id| {
            let title = title_for(&id, blueprint);
            Story { id, title }
        })
        .collect()
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

/// The story ID an issue title names, when the title carries one.
///
/// The ID is the text before the first colon, and only an ID-shaped token
/// counts, so a tracker full of ordinary issues contributes no orphans.
pub(super) fn key_of(title: &str) -> Option<&str> {
    let (key, _) = title.split_once(':')?;
    let key = key.trim();
    if key.is_empty() || key.len() > 64 {
        return None;
    }
    let shaped = key
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    let has_letter = key.chars().any(|c| c.is_ascii_alphabetic());
    (shaped && has_letter).then_some(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_core::ir::{Expr, HttpMethod, RequestSpec, ResponseSpec, RouteDefinition, TypeRef};

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
        let stories = from_blueprint(&blueprint);
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
        let stories = from_blueprint(&blueprint);
        assert_eq!(stories[0].title, "US-002: GET /ping; POST /echo");
    }

    #[test]
    fn a_title_round_trips_through_the_key() {
        let title = "US-001: GET /ping";
        assert_eq!(key_of(title), Some("US-001"));
        assert_eq!(
            key_of("ORD-7"),
            None,
            "an issue without a colon is no story"
        );
        assert_eq!(key_of("Fix the flaky test: again"), None, "not ID-shaped");
        assert_eq!(key_of(": no key"), None);
        assert_eq!(key_of("US-001: GET /ping; POST /echo"), Some("US-001"));
    }
}
