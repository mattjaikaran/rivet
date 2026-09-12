//! Story-to-code gate (`E2045`): every public route carries a story ID.
//!
//! Pillar 05 (`docs/pillars/05-story-to-code-traceability.md`) ties every
//! endpoint to a user story so audits can trace requirements to code:
//!
//! ```python
//! @api.post("/orders", stories=["US-123", "EPIC-456"])
//! ```
//!
//! A route whose decorator carries no `stories=[...]` fails the build with
//! a fix suggestion. The project opts out by setting
//! `stories_required = false` in the `[verifier]` config.

use crate::parser::python::DeclKind;
use crate::verifier::{Context, Finding, Rule};
use std::collections::HashMap;

pub(crate) struct StoryLink;

impl Rule for StoryLink {
    fn id(&self) -> &'static str {
        "story_link"
    }

    fn default_severity(&self) -> crate::diagnostic::Severity {
        crate::diagnostic::Severity::Blocker
    }

    fn check(&self, ctx: &Context<'_>) -> Vec<Finding> {
        // Handler name -> declaration line, so the finding points at the
        // def line instead of falling back to line 1.
        let handler_lines: HashMap<&str, usize> = ctx
            .module
            .decls
            .iter()
            .filter(|decl| decl.kind == DeclKind::Route)
            .map(|decl| (decl.name.as_str(), decl.line))
            .collect();

        let mut findings = Vec::new();
        for route in &ctx.module.blueprint.routes {
            if !route.stories.is_empty() {
                continue;
            }
            let line = handler_lines
                .get(route.handler_name.as_str())
                .copied()
                .unwrap_or(1);
            let method = route.method.axum_router_fn();
            findings.push(
                Finding::new(
                    "E2045",
                    ctx.severity,
                    format!(
                        "route `{}` ({} {}) has no story link; every public endpoint needs one",
                        route.handler_name, method, route.path
                    ),
                    &ctx.module.file,
                    line,
                )
                .at_path(route.handler_name.clone())
                .with_fix(format!(
                    "add a stories argument to the decorator, for example @api.{method}(\"{}\", stories=[\"US-1\"])",
                    route.path
                )),
            );
        }
        findings
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::VerifierConfig;
    use crate::parser::python::parse_python_module;
    use crate::verifier::run_rule;

    fn module(source: &str) -> crate::parser::python::ParsedModule {
        parse_python_module(source, "app", "app.py").expect("module must parse")
    }

    #[test]
    fn route_without_stories_fails_with_a_fix_suggestion() {
        let parsed = module(
            r#"
from rivet import api

@api.get("/ping")
def ping() -> dict:
    return {"status": "pong"}
"#,
        );
        let diagnostics = run_rule(&parsed, &VerifierConfig::default(), &StoryLink);
        assert_eq!(diagnostics.len(), 1);
        let diagnostic = &diagnostics[0];
        assert_eq!(diagnostic.error_code, "E2045");
        assert_eq!(diagnostic.line, Some(4));
        let fix = diagnostic.suggested_fix.as_str();
        assert!(!fix.is_empty(), "E2045 must carry a non-empty fix");
        assert!(fix.contains("@api.get(\"/ping\""), "fix: {fix}");
        assert!(fix.contains("stories"), "fix: {fix}");
    }

    #[test]
    fn tagged_routes_pass_the_default_gate() {
        let parsed = module(
            r#"
from rivet import api

@api.post("/orders", stories=["US-123"])
def create_order(request: dict) -> dict:
    return {"echo": request}
"#,
        );
        let diagnostics = run_rule(&parsed, &VerifierConfig::default(), &StoryLink);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn opt_out_allows_storyless_routes() {
        let parsed = module(
            r#"
from rivet import api

@api.get("/ping")
def ping() -> dict:
    return {"status": "pong"}
"#,
        );
        let config = VerifierConfig {
            stories_required: false,
            ..VerifierConfig::default()
        };
        let diagnostics = run_rule(&parsed, &config, &StoryLink);
        assert!(diagnostics.is_empty());
    }
}
