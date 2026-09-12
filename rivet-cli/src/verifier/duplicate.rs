//! Duplicate-code rule (`E2043`): handlers that share an implementation.
//!
//! A handler's implementation fingerprint is its parameter list, return
//! annotation, and body text, whitespace-normalized. The route name, path,
//! and decorators are excluded, so two routes that differ only by name and
//! URL still collide — that is exactly the redundancy this rule exists to
//! catch. Every group of two or more identical implementations produces one
//! finding that names each handler and its line; the outcome is
//! `[verifier] duplicate_code` (blocker by default).

use crate::verifier::{Context, Finding, Rule};
use crate::parser::NamedChildren;
use crate::parser::python::DeclKind;
use std::collections::HashMap;
use tree_sitter::Node;

pub(crate) struct Duplicate;

/// Collapse whitespace so indentation differences do not hide a duplicate.
fn normalize(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The implementation fingerprint of a route's function definition.
fn fingerprint(function: &Node<'_>, source: &str) -> String {
    let mut parts = Vec::new();
    for field in ["parameters", "return_type", "body"] {
        if let Some(node) = function.child_by_field_name(field) {
            parts.push(normalize(&source[node.start_byte()..node.end_byte()]));
        }
    }
    parts.join("|")
}

impl Rule for Duplicate {
    fn id(&self) -> &'static str {
        "duplicate_code"
    }

    fn default_severity(&self) -> crate::diagnostic::Severity {
        crate::diagnostic::Severity::Blocker
    }

    fn check(&self, ctx: &Context<'_>) -> Vec<Finding> {
        let root = ctx.module.tree.root_node();
        // fingerprint -> (first appearance order, member handlers).
        let mut groups: HashMap<String, (usize, Vec<(String, usize)>)> = HashMap::new();
        let mut order: Vec<String> = Vec::new();
        for node in root.named_children_all() {
            let Some(decl) = ctx.module.decl_at(node.start_byte()) else {
                continue;
            };
            if decl.kind != DeclKind::Route {
                continue;
            }
            let Some(function) = node
                .named_children_all()
                .into_iter()
                .find(|child| child.kind() == "function_definition")
            else {
                continue;
            };
            let key = fingerprint(&function, &ctx.module.source);
            let entry = groups.entry(key.clone()).or_insert_with(|| {
                order.push(key.clone());
                (order.len() - 1, Vec::new())
            });
            entry.1.push((decl.name.clone(), decl.line));
        }

        let mut findings = Vec::new();
        let mut keys: Vec<&String> = order.iter().collect();
        keys.sort_by_key(|key| groups[*key].0);
        for key in keys {
            let (_, members) = &groups[key];
            if members.len() < 2 {
                continue;
            }
            let first = &members[0];
            let names: Vec<String> = members
                .iter()
                .map(|(name, line)| format!("`{name}` (line {line})"))
                .collect();
            findings.push(
                Finding::new(
                    "E2043",
                    ctx.severity,
                    format!(
                        "{} handlers have identical implementations: {}",
                        members.len(),
                        names.join(", ")
                    ),
                    &ctx.module.file,
                    first.1,
                )
                .at_path(first.0.clone())
                .with_fix(
                    "extract the shared logic into one handler or a reusable helper, and route the other paths to it",
                ),
            );
        }
        findings
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::VerifierConfig;
    use crate::diagnostic::Severity;
    use crate::verifier::run_rule;
    use crate::parser::python::parse_python_module;

    fn module(source: &str) -> crate::parser::python::ParsedModule {
        parse_python_module(source, "app", "app.py").expect("module must parse")
    }

    const DUPLICATES: &str = r#"
from rivet import api

@api.get("/a", stories=["US-1"])
def alpha() -> dict:
    return {"kind": "same"}

@api.post("/b", stories=["US-1"])
def beta() -> dict:
    return {"kind": "same"}
"#;

    #[test]
    fn identical_handlers_produce_one_finding_naming_both() {
        let parsed = module(DUPLICATES);
        let diagnostics = run_rule(&parsed, &VerifierConfig::default(), &Duplicate);
        assert_eq!(diagnostics.len(), 1);
        let diagnostic = &diagnostics[0];
        assert_eq!(diagnostic.error_code, "E2043");
        assert_eq!(diagnostic.line, Some(4));
        let message = &diagnostic.message;
        assert!(message.contains("`alpha` (line 4)"), "message: {message}");
        assert!(message.contains("`beta` (line 8)"), "message: {message}");
        assert!(
            !diagnostic.suggested_fix.is_empty(),
            "E2043 must carry a non-empty fix"
        );
    }

    #[test]
    fn distinct_handlers_pass() {
        let parsed = module(
            r#"
from rivet import api

@api.get("/ping", stories=["US-1"])
def ping() -> dict:
    return {"status": "pong"}

@api.post("/echo", stories=["US-1"])
def echo(request: dict) -> dict:
    return {"echo": request}
"#,
        );
        let diagnostics = run_rule(&parsed, &VerifierConfig::default(), &Duplicate);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn warning_outcome_reports_instead_of_blocking() {
        let parsed = module(DUPLICATES);
        let config = VerifierConfig {
            duplicate_code: Severity::Warning,
            ..VerifierConfig::default()
        };
        let diagnostics = run_rule(&parsed, &config, &Duplicate);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].severity, Severity::Warning);
    }
}
