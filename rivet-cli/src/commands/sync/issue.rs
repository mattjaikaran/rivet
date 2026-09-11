//! The story diff: what the blueprint declares against what the tracker
//! holds.
//!
//! Four disagreements are reported, and nothing else:
//!
//! - **missing** — the blueprint declares a story the tracker has no issue
//!   for. `--apply` creates those issues.
//! - **orphan** — the tracker holds an issue for a story the blueprint no
//!   longer declares.
//! - **title drift** — the issue title no longer matches the routes the
//!   blueprint serves for that story.
//! - **state drift** — the tracker closed the issue while the blueprint
//!   still serves the story.
//!
//! The computation is pure: it takes the stories and the issues and returns
//! the diff, so a captured tracker payload exercises it offline.

use super::story::{Story, key_of};

/// One issue as the tracker reported it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Issue {
    /// The tracker's own key, for example `ORD-7`.
    pub(super) key: String,
    /// The issue title.
    pub(super) title: String,
    /// Whether the tracker considers the issue finished.
    pub(super) closed: bool,
}

/// Why an issue and the blueprint disagree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DriftKind {
    /// The issue title does not match the routes the story covers.
    Title,
    /// The tracker closed an issue whose story is still served.
    State,
}

impl DriftKind {
    /// The report label.
    pub(super) fn as_str(self) -> &'static str {
        match self {
            DriftKind::Title => "title drift",
            DriftKind::State => "state drift",
        }
    }
}

/// One drifted issue, with the story it disagrees with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Drift {
    /// The kind of disagreement.
    pub(super) kind: DriftKind,
    /// The story ID the issue names.
    pub(super) story: String,
    /// The tracker key.
    pub(super) issue: String,
    /// What the tool found, in one line.
    pub(super) detail: String,
}

/// The reconciliation between the blueprint and the tracker.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Diff {
    /// Stories with no issue.
    pub(super) missing: Vec<Story>,
    /// Issues naming a story the blueprint does not declare.
    pub(super) orphan: Vec<Issue>,
    /// Issues that disagree with their story.
    pub(super) drifted: Vec<Drift>,
}

impl Diff {
    /// Whether the blueprint and the tracker agree.
    pub(super) fn is_empty(&self) -> bool {
        self.missing.is_empty() && self.orphan.is_empty() && self.drifted.is_empty()
    }

    /// One line per difference, in report order.
    pub(super) fn lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        for story in &self.missing {
            lines.push(format!("missing issue: {} ({})", story.id, story.title));
        }
        for issue in &self.orphan {
            lines.push(format!(
                "orphan issue: {} ({}) names no story in the blueprint",
                issue.key, issue.title
            ));
        }
        for drift in &self.drifted {
            lines.push(format!(
                "{}: {} — {}",
                drift.kind.as_str(),
                drift.issue,
                drift.detail
            ));
        }
        lines
    }

    /// The one-line summary of the diff.
    pub(super) fn summary(&self) -> String {
        if self.is_empty() {
            return "the blueprint and the tracker agree".to_string();
        }
        format!(
            "{} missing, {} orphan, {} drifted",
            self.missing.len(),
            self.orphan.len(),
            self.drifted.len()
        )
    }
}

/// Compute the diff between the stories and the issues.
pub(super) fn compute(stories: &[Story], issues: &[Issue]) -> Diff {
    let mut diff = Diff::default();

    for issue in issues {
        let Some(key) = key_of(&issue.title) else {
            continue;
        };
        let Some(story) = stories.iter().find(|story| story.id == key) else {
            diff.orphan.push(issue.clone());
            continue;
        };
        if issue.title.trim() != story.title {
            diff.drifted.push(Drift {
                kind: DriftKind::Title,
                story: story.id.clone(),
                issue: issue.key.clone(),
                detail: format!(
                    "{} is titled {:?}, and the blueprint serves {:?}",
                    story.id, issue.title, story.title
                ),
            });
        }
        if issue.closed {
            diff.drifted.push(Drift {
                kind: DriftKind::State,
                story: story.id.clone(),
                issue: issue.key.clone(),
                detail: format!(
                    "{} is closed in the tracker and the blueprint still serves it",
                    story.id
                ),
            });
        }
    }

    for story in stories {
        let tracked = issues
            .iter()
            .any(|issue| key_of(&issue.title) == Some(story.id.as_str()));
        if !tracked {
            diff.missing.push(story.clone());
        }
    }

    diff
}

#[cfg(test)]
mod tests {
    use super::*;

    fn story(id: &str, title: &str) -> Story {
        Story {
            id: id.to_string(),
            title: title.to_string(),
        }
    }

    fn issue(key: &str, title: &str, closed: bool) -> Issue {
        Issue {
            key: key.to_string(),
            title: title.to_string(),
            closed,
        }
    }

    #[test]
    fn an_agreeing_tracker_produces_an_empty_diff() {
        let stories = vec![story("US-001", "US-001: GET /ping")];
        let issues = vec![issue("ORD-1", "US-001: GET /ping", false)];
        let diff = compute(&stories, &issues);
        assert!(diff.is_empty(), "{diff:?}");
        assert_eq!(diff.summary(), "the blueprint and the tracker agree");
        assert!(diff.lines().is_empty());
    }

    #[test]
    fn a_story_without_an_issue_is_missing() {
        let stories = vec![
            story("US-001", "US-001: GET /ping"),
            story("US-002", "US-002: POST /echo"),
        ];
        let issues = vec![issue("ORD-1", "US-001: GET /ping", false)];
        let diff = compute(&stories, &issues);
        assert_eq!(diff.missing.len(), 1);
        assert_eq!(diff.missing[0].id, "US-002");
        assert!(diff.orphan.is_empty());
        assert!(diff.drifted.is_empty());
    }

    #[test]
    fn an_issue_for_an_undeclared_story_is_an_orphan() {
        let stories = vec![story("US-001", "US-001: GET /ping")];
        let issues = vec![
            issue("ORD-1", "US-001: GET /ping", false),
            issue("ORD-9", "US-999: POST /gone", false),
        ];
        let diff = compute(&stories, &issues);
        assert_eq!(diff.orphan.len(), 1);
        assert_eq!(diff.orphan[0].key, "ORD-9");
    }

    #[test]
    fn an_issue_title_that_no_longer_matches_the_routes_drifts() {
        let stories = vec![story("US-001", "US-001: GET /ping; POST /ping")];
        let issues = vec![issue("ORD-1", "US-001: GET /ping", false)];
        let diff = compute(&stories, &issues);
        assert_eq!(diff.drifted.len(), 1);
        assert_eq!(diff.drifted[0].kind, DriftKind::Title);
        assert!(diff.drifted[0].detail.contains("POST /ping"));
    }

    #[test]
    fn a_closed_issue_for_a_served_story_drifts() {
        let stories = vec![story("US-001", "US-001: GET /ping")];
        let issues = vec![issue("ORD-1", "US-001: GET /ping", true)];
        let diff = compute(&stories, &issues);
        assert_eq!(diff.drifted.len(), 1);
        assert_eq!(diff.drifted[0].kind, DriftKind::State);
        assert!(diff.missing.is_empty(), "the story is tracked");
    }

    #[test]
    fn an_issue_that_names_no_story_is_ignored() {
        let stories = vec![story("US-001", "US-001: GET /ping")];
        let issues = vec![
            issue("ORD-1", "US-001: GET /ping", false),
            issue("ORD-2", "Fix the flaky test: again", true),
        ];
        let diff = compute(&stories, &issues);
        assert!(diff.is_empty(), "{diff:?}");
    }

    #[test]
    fn both_drifts_are_reported_for_one_issue() {
        let stories = vec![story("US-001", "US-001: GET /ping")];
        let issues = vec![issue("ORD-1", "US-001: POST /echo", true)];
        let diff = compute(&stories, &issues);
        assert_eq!(diff.drifted.len(), 2);
        assert_eq!(diff.drifted[0].kind, DriftKind::Title);
        assert_eq!(diff.drifted[1].kind, DriftKind::State);
        assert_eq!(diff.summary(), "0 missing, 0 orphan, 2 drifted");
    }
}
