//! Dead-code rule (`E2044`): helpers and DTOs that nothing references.
//!
//! A helper function is live when another top-level function or class in
//! the module refers to it by name from its own body. Route bodies cannot
//! call helpers yet, so today a helper only stays alive by serving another
//! helper. A DTO is live when a route request or response type reaches it
//! (the blueprint carries exactly the reachable closure, so any declared
//! DTO missing from it is unreferenced).
//!
//! Findings report at `[verifier] dead_code` severity (warning by default)
//! so a build can carry reported debt without breaking.

use crate::verifier::{Context, Finding, Rule, named_children};
use crate::parser::python::{DeclKind, ParsedModule};
use std::collections::HashMap;
use tree_sitter::Node;

pub(crate) struct DeadCode;

/// Whether a subtree mentions a helper name. The excluded set holds the
/// names a function shadows (its own name and its parameters), so a
/// self-reference or a shadowing parameter never counts as a use.
fn subtree_mentions(node: Node<'_>, source: &str, excluded: &[String], needle: &str) -> bool {
    if node.kind() == "identifier" {
        let name = &source[node.start_byte()..node.end_byte()];
        if name == needle && !excluded.iter().any(|shadow| shadow == needle) {
            return true;
        }
    }
    named_children(node)
        .into_iter()
        .any(|child| subtree_mentions(child, source, excluded, needle))
}

/// The names a definition shadows from reference counting: its own name and
/// its parameters. Only the definition header is inspected, never the body.
fn shadowed_names(node: Node<'_>, source: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut definition = node;
    if node.kind() == "decorated_definition" {
        let Some(inner) = named_children(node)
            .into_iter()
            .find(|child| matches!(child.kind(), "function_definition" | "class_definition"))
        else {
            return names;
        };
        definition = inner;
    }
    let mut header = Vec::new();
    if let Some(name) = definition.child_by_field_name("name") {
        header.push(name);
    }
    if let Some(parameters) = definition.child_by_field_name("parameters") {
        let mut stack: Vec<Node<'_>> = named_children(parameters);
        while let Some(current) = stack.pop() {
            header.push(current);
            stack.extend(named_children(current));
        }
    }
    for node in header {
        if node.kind() == "identifier" {
            names.push(source[node.start_byte()..node.end_byte()].to_string());
        }
    }
    names
}

impl Rule for DeadCode {
    fn id(&self) -> &'static str {
        "dead_code"
    }

    fn default_severity(&self) -> crate::diagnostic::Severity {
        crate::diagnostic::Severity::Warning
    }

    fn check(&self, ctx: &Context<'_>) -> Vec<Finding> {
        let module: &ParsedModule = ctx.module;
        let root = module.tree.root_node();
        let helper_names: Vec<String> = module
            .decls
            .iter()
            .filter(|decl| decl.kind == DeclKind::Helper)
            .map(|decl| decl.name.clone())
            .collect();

        // Which helper each top-level definition mentions, keyed by the
        // definition's own name so a definition never marks itself live.
        let mut mentions: HashMap<String, Vec<String>> = HashMap::new();
        for node in named_children(root) {
            let Some(decl) = module.decl_at(node.start_byte()) else {
                continue;
            };
            if !matches!(
                decl.kind,
                DeclKind::Helper
                    | DeclKind::Foreign
                    | DeclKind::Dto
                    | DeclKind::RuntimeClass
                    | DeclKind::Other
            ) {
                continue;
            }
            let excluded = shadowed_names(node, &module.source);
            let mut seen = Vec::new();
            for helper in &helper_names {
                if subtree_mentions(node, &module.source, &excluded, helper) {
                    seen.push(helper.clone());
                }
            }
            mentions.insert(decl.name.clone(), seen);
        }

        let mut findings = Vec::new();
        let live: Vec<String> = mentions
            .values()
            .flat_map(|seen| seen.iter().cloned())
            .collect();
        let struct_names: Vec<String> = module
            .blueprint
            .structs
            .iter()
            .map(|structure| structure.name.clone())
            .collect();
        for decl in &module.decls {
            match decl.kind {
                DeclKind::Helper if !live.contains(&decl.name) => findings.push(
                    Finding::new(
                        "E2044",
                        ctx.severity,
                        format!(
                            "helper function `{}` is never called by another module function",
                            decl.name
                        ),
                        &module.file,
                        decl.line,
                    )
                    .at_path(decl.name.clone())
                    .with_fix("remove the helper, or call it from a handler or another helper"),
                ),
                DeclKind::Dto if !struct_names.contains(&decl.name) => findings.push(
                    Finding::new(
                        "E2044",
                        ctx.severity,
                        format!("DTO `{}` is not referenced by any route", decl.name),
                        &module.file,
                        decl.line,
                    )
                    .at_path(decl.name.clone())
                    .with_fix("remove the class, or use it in a route request or response type"),
                ),
                _ => {}
            }
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

    const WITH_HELPER: &str = r#"
from rivet import api

def unused(value: int) -> int:
    return value + 1

class Ghost:
    id: int

@api.get("/ping", stories=["US-1"])
def ping() -> dict:
    return {"status": "pong"}
"#;

    #[test]
    fn unused_helper_and_dto_warn_by_default() {
        let parsed = module(WITH_HELPER);
        let diagnostics = run_rule(&parsed, &VerifierConfig::default(), &DeadCode);
        assert_eq!(diagnostics.len(), 2);
        assert!(diagnostics.iter().all(|d| d.error_code == "E2044"));
        assert!(diagnostics.iter().all(|d| d.severity == Severity::Warning));
        let messages: Vec<&str> = diagnostics.iter().map(|d| d.message.as_str()).collect();
        assert!(messages.iter().any(|m| m.contains("unused")));
        assert!(messages.iter().any(|m| m.contains("Ghost")));
        assert!(
            diagnostics.iter().all(|d| !d.suggested_fix.is_empty()),
            "every E2044 finding must carry a non-empty fix"
        );
    }

    #[test]
    fn configured_blocker_outcome_stops_the_build() {
        let parsed = module(WITH_HELPER);
        let config = VerifierConfig {
            dead_code: Severity::Blocker,
            ..VerifierConfig::default()
        };
        let diagnostics = run_rule(&parsed, &config, &DeadCode);
        assert_eq!(diagnostics.len(), 2);
        assert!(diagnostics.iter().all(|d| d.severity == Severity::Blocker));
    }

    #[test]
    fn helper_called_by_another_helper_is_live() {
        let source = r#"
from rivet import api

def format_price(value: int) -> int:
    return value * 2

def render() -> str:
    return str(format_price(2))

@api.get("/ping", stories=["US-1"])
def ping() -> dict:
    return {"status": "pong"}
"#;
        let parsed = module(source);
        let diagnostics = run_rule(&parsed, &VerifierConfig::default(), &DeadCode);
        // Only `render` is dead; `format_price` is called by it.
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("render"));
    }

    #[test]
    fn dto_referenced_by_a_route_is_live() {
        let source = r#"
from rivet import api

class Order:
    sku: str

@api.post("/orders", stories=["US-1"])
def create(request: Order) -> dict:
    return {"ok": True}
"#;
        let parsed = module(source);
        let diagnostics = run_rule(&parsed, &VerifierConfig::default(), &DeadCode);
        assert!(diagnostics.is_empty());
    }
}
