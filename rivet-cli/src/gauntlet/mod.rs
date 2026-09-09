//! The Gauntlet: compile-time quality rules that run between parse and
//! generate.
//!
//! Each rule is one small module beside this one, mirroring the parser
//! layout. A rule inspects the parsed module (its syntax tree, declaration
//! table, and blueprint) and returns [`Finding`]s. The orchestrator turns
//! findings into [`Diagnostic`]s on the standard JSON path: blockers stop
//! the build, warnings report and let it continue.
//!
//! Severity is policy: the `[gauntlet]` config decides which rules run and
//! at which severity (see [`crate::config::GauntletConfig`]). Rules that
//! guard a hard guarantee (`complexity`, `story_link`, `type_strictness`)
//! are blockers when enabled.
//!
//! Error codes owned by the Gauntlet:
//!
//! | code | rule | meaning |
//! | --- | --- | --- |
//! | E2042 | [`complexity`] | cyclomatic complexity above `max_complexity` |
//! | E2043 | [`duplicate`] | handlers that share an implementation |
//! | E2044 | [`dead_code`] | helper or DTO that nothing references |
//! | E2045 | [`story_link`] | route without a story ID (pillar 05) |
//! | E2046 | [`type_strict`] | module construct the engine cannot translate |
//!
//! The agentic-JSON shape follows `docs/pillars/07-the-gauntlet.md`.

use crate::config::GauntletConfig;
use crate::diagnostic::{Diagnostic, Severity};
use crate::parser::python::ParsedModule;
use tree_sitter::Node;

pub mod complexity;
pub mod dead_code;
pub mod duplicate;
pub mod story_link;
pub mod type_strict;

/// Named children of a node, tied to the tree lifetime.
///
/// The parser's `NamedChildren` trait ties children to the parent borrow,
/// which is fine for immediate iteration but not for rules that hoard nodes
/// from several levels into one container. Walking a cursor rooted at the
/// node yields tree-lifetime nodes instead.
pub(crate) fn named_children<'t>(node: Node<'t>) -> Vec<Node<'t>> {
    let mut cursor = node.walk();
    let mut children = Vec::new();
    if cursor.goto_first_child() {
        loop {
            let child = cursor.node();
            if child.is_named() {
                children.push(child);
            }
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
    children
}

/// One rule result, anchored to a source location.
///
/// Rules build findings for every violation and let the orchestrator decide
/// the outcome; nothing is filtered at the rule boundary.
#[derive(Debug, Clone)]
pub struct Finding {
    pub code: &'static str,
    pub severity: Severity,
    pub message: String,
    pub file: String,
    pub line: usize,
    pub ast_path: Option<String>,
    pub suggested_fix: Option<String>,
}

impl Finding {
    pub fn new(
        code: &'static str,
        severity: Severity,
        message: impl Into<String>,
        file: &str,
        line: usize,
    ) -> Self {
        Self {
            code,
            severity,
            message: message.into(),
            file: file.to_string(),
            line,
            ast_path: None,
            suggested_fix: None,
        }
    }

    /// Name the syntax node the finding points at, for agents.
    pub fn at_path(mut self, path: impl Into<String>) -> Self {
        self.ast_path = Some(path.into());
        self
    }

    pub fn with_fix(mut self, fix: impl Into<String>) -> Self {
        self.suggested_fix = Some(fix.into());
        self
    }

    pub fn into_diagnostic(self) -> Diagnostic {
        Diagnostic {
            error_code: self.code.to_string(),
            severity: self.severity,
            message: self.message,
            file: Some(self.file),
            line: Some(self.line),
            column: None,
            suggested_fix: self.suggested_fix,
            ast_path: self.ast_path,
        }
    }
}

/// Everything a rule needs to inspect one module.
pub(crate) struct Context<'a> {
    pub module: &'a ParsedModule,
    pub config: &'a GauntletConfig,
    /// The severity the config resolved for this rule.
    pub severity: Severity,
}

pub(crate) trait Rule: Sync {
    /// Stable config key, e.g. `"dead_code"`.
    fn id(&self) -> &'static str;
    /// Severity when the config does not name this rule.
    fn default_severity(&self) -> Severity;
    /// Inspect the module and return every violation found.
    fn check(&self, ctx: &Context<'_>) -> Vec<Finding>;
}

/// The rule set, in output order. Complexity runs first so the most severe
/// structural finding leads; the rest follow the E204x code order.
static RULES: &[&dyn Rule] = &[
    &complexity::Complexity,
    &duplicate::Duplicate,
    &dead_code::DeadCode,
    &story_link::StoryLink,
    &type_strict::TypeStrict,
];

/// Run every enabled rule over a parsed module.
///
/// Findings come back as diagnostics sorted by source line so the output is
/// stable. The caller separates blockers (fail the build) from warnings
/// (report and continue).
pub fn run_gauntlet(module: &ParsedModule, config: &GauntletConfig) -> Vec<Diagnostic> {
    run_rules(module, config, RULES)
}

/// Run a specific rule list. Tests use this to exercise one rule or a
/// stand-in; production goes through [`run_gauntlet`].
fn run_rules(
    module: &ParsedModule,
    config: &GauntletConfig,
    rules: &[&dyn Rule],
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for rule in rules {
        let Some(severity) = policy_for(*rule, config) else {
            continue; // rule disabled by config
        };
        let ctx = Context {
            module,
            config,
            severity,
        };
        for finding in rule.check(&ctx) {
            diagnostics.push(finding.into_diagnostic());
        }
    }
    diagnostics.sort_by_key(|diagnostic| diagnostic.line);
    diagnostics
}

/// Run one rule. Tests use this instead of building a one-element rule
/// list.
#[cfg(test)]
pub(crate) fn run_rule(
    module: &ParsedModule,
    config: &GauntletConfig,
    rule: &dyn Rule,
) -> Vec<Diagnostic> {
    run_rules(module, config, &[rule])
}

/// Resolve the outcome for a rule from the config.
///
/// Rules with hard guarantees are blockers when enabled; `duplicate_code`
/// and `dead_code` take their severity from the config.
fn policy_for(rule: &dyn Rule, config: &GauntletConfig) -> Option<Severity> {
    match rule.id() {
        "complexity" => Some(Severity::Blocker),
        "story_link" => config.stories_required.then_some(Severity::Blocker),
        "type_strictness" => config.strict_type_checking.then_some(Severity::Blocker),
        "duplicate_code" => Some(config.duplicate_code),
        "dead_code" => Some(config.dead_code),
        _ => Some(rule.default_severity()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::python::parse_python_module;

    /// The smallest module a rule can inspect: one story-tagged route.
    const MINIMAL: &str = r#"
from rivet import api

@api.get("/ping", stories=["US-1"])
def ping() -> dict:
    return {"status": "pong"}
"#;

    /// A registered stand-in rule so tests cover the harness, not a rule.
    struct AlwaysBlock;
    impl Rule for AlwaysBlock {
        fn id(&self) -> &'static str {
            "test_rule"
        }
        fn default_severity(&self) -> Severity {
            Severity::Blocker
        }
        fn check(&self, ctx: &Context<'_>) -> Vec<Finding> {
            vec![Finding::new(
                "E2999",
                ctx.severity,
                "test finding",
                &ctx.module.file,
                2,
            )]
        }
    }

    fn module() -> ParsedModule {
        parse_python_module(MINIMAL, "app", "app.py").expect("module must parse")
    }

    #[test]
    fn registered_rule_findings_become_json_diagnostics() {
        let parsed = module();
        let diagnostics = run_rule(&parsed, &GauntletConfig::default(), &AlwaysBlock);
        assert_eq!(diagnostics.len(), 1);
        let json: serde_json::Value =
            serde_json::from_str(&diagnostics[0].to_json()).expect("payload must parse");
        assert_eq!(json["error_code"], "E2999");
        assert_eq!(json["severity"], "blocker");
        assert_eq!(json["file"], "app.py");
        assert_eq!(json["line"], 2);
    }

    #[test]
    fn unknown_rule_falls_back_to_its_default_severity() {
        let parsed = module();
        let diagnostics = run_rule(&parsed, &GauntletConfig::default(), &AlwaysBlock);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].severity, Severity::Blocker);
    }

    #[test]
    fn disabled_story_rule_produces_no_findings() {
        let parsed = module();
        let config = GauntletConfig {
            stories_required: false,
            ..GauntletConfig::default()
        };
        let diagnostics = run_rule(&parsed, &config, &story_link::StoryLink);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn findings_sort_by_source_line() {
        let source = r#"
from rivet import api

@api.post("/a", stories=["US-1"])
def a(request: dict) -> dict:
    return {"echo": request}

@api.post("/b")
def b(request: dict) -> dict:
    return {"echo": request}
"#;
        let parsed = parse_python_module(source, "app", "app.py").expect("module must parse");
        let rules: &[&dyn Rule] = &[&duplicate::Duplicate, &story_link::StoryLink];
        let diagnostics = run_rules(&parsed, &GauntletConfig::default(), rules);
        let lines: Vec<usize> = diagnostics.iter().map(|d| d.line.unwrap_or(0)).collect();
        // The duplicate fires on line 4 (first shared body), the missing
        // story on line 8; sorted output must lead with the earlier line.
        assert_eq!(lines, vec![4, 8]);
        assert_eq!(diagnostics[0].error_code, "E2043");
        assert_eq!(diagnostics[1].error_code, "E2045");
    }
}
