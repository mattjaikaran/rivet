//! Complexity rule (`E2042`): cyclomatic complexity over handler bodies and
//! DTO classes.
//!
//! Metric: 1 (base) plus one point per decision node in the body:
//!
//! - `if_statement` and each `elif_clause`
//! - `for_statement` and `while_statement`
//! - `boolean_operator` (`and` / `or`)
//! - `conditional_expression` (`x if c else y`)
//! - `case_clause` of a `match`
//!
//! `else` arms and plain blocks add nothing. The walker counts nodes in the
//! body subtree, so nested branches accumulate. A handler whose score
//! exceeds `[gauntlet] max_complexity` fails the build; the `ast_path` names
//! the decision that crossed the limit, for example
//! `create_order.if_statement.2`.
//!
//! Phase-0 handler bodies hold a single return (score 1), so real modules
//! only trip this rule once control flow lands in the DSL; the walker and
//! its contract are exercised directly against the syntax tree.

use crate::gauntlet::{Context, Finding, Rule, named_children};
use crate::parser::python::{DeclKind, Declaration, ParsedModule};
use tree_sitter::Node;

pub(crate) struct Complexity;

/// The canonical name of a decision node kind, or `None` for other nodes.
///
/// `Node::kind` borrows the node, so decision tracking works with these
/// `'static` names instead.
fn decision_kind(kind: &str) -> Option<&'static str> {
    match kind {
        "if_statement" => Some("if_statement"),
        "elif_clause" => Some("elif_clause"),
        "for_statement" => Some("for_statement"),
        "while_statement" => Some("while_statement"),
        "boolean_operator" => Some("boolean_operator"),
        "conditional_expression" => Some("conditional_expression"),
        "case_clause" => Some("case_clause"),
        _ => None,
    }
}

/// One decision point in document order, with its ordinal within its kind.
struct Decision {
    kind: &'static str,
    ordinal: usize,
}

/// Collect the decision points in a subtree, in document order.
fn decisions_in(
    node: Node<'_>,
    out: &mut Vec<Decision>,
    ordinals: &mut std::collections::HashMap<&'static str, usize>,
) {
    if let Some(kind) = decision_kind(node.kind()) {
        let ordinal = ordinals.entry(kind).or_insert(0);
        *ordinal += 1;
        out.push(Decision {
            kind,
            ordinal: *ordinal,
        });
    }
    for child in named_children(node) {
        decisions_in(child, out, ordinals);
    }
}

/// The block a declaration's logic lives in: the function or class body.
fn logic_block<'m>(module: &'m ParsedModule, decl: &Declaration) -> Option<Node<'m>> {
    let root = module.tree.root_node();
    let node = named_children(root)
        .into_iter()
        .find(|node| node.start_byte() == decl.start_byte)?;
    let definition = match decl.kind {
        DeclKind::Route => named_children(node)
            .into_iter()
            .find(|child| child.kind() == "function_definition")?,
        DeclKind::Dto => node,
        _ => return None,
    };
    definition.child_by_field_name("body")
}

impl Rule for Complexity {
    fn id(&self) -> &'static str {
        "complexity"
    }

    fn default_severity(&self) -> crate::diagnostic::Severity {
        crate::diagnostic::Severity::Blocker
    }

    fn check(&self, ctx: &Context<'_>) -> Vec<Finding> {
        let max = ctx.config.max_complexity;
        let mut findings = Vec::new();
        for decl in &ctx.module.decls {
            if !matches!(decl.kind, DeclKind::Route | DeclKind::Dto) {
                continue;
            }
            let Some(body) = logic_block(ctx.module, decl) else {
                continue;
            };
            let mut decisions = Vec::new();
            decisions_in(body, &mut decisions, &mut std::collections::HashMap::new());
            let total = 1 + decisions.len();
            if total <= max {
                continue;
            }
            // The decision that pushed the score past the limit: for a
            // limit of 8 the eighth decision (index 7) crosses it.
            let crossing = decisions
                .get(max.saturating_sub(1))
                .or_else(|| decisions.last());
            let path = match crossing {
                Some(decision) => format!("{}.{}.{}", decl.name, decision.kind, decision.ordinal),
                None => decl.name.clone(),
            };
            findings.push(
                Finding::new(
                    "E2042",
                    ctx.severity,
                    format!(
                        "cyclomatic complexity of `{}` is {total}, above the maximum of {max}",
                        decl.name
                    ),
                    &ctx.module.file,
                    decl.line,
                )
                .at_path(path)
                .with_fix(format!(
                    "split `{}` into smaller handlers or helpers so each body stays under {max} decision points",
                    decl.name
                )),
            );
        }
        findings
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gauntlet::run_gauntlet;
    use crate::parser::NamedChildren;
    use crate::parser::node_text;
    use rivet_core::ir::ServiceBlueprint;
    use tree_sitter::Parser;

    /// Parse a module's syntax tree without running the strict body
    /// lowering, so a handler body may hold control flow. Decorated
    /// functions classify as routes; plain ones as helpers.
    fn raw_module(source: &str) -> ParsedModule {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_python::LANGUAGE.into())
            .expect("grammar loads");
        let tree = parser.parse(source, None).expect("source parses");
        let root = tree.root_node();
        let mut decls: Vec<Declaration> = Vec::new();
        for node in root.named_children_all() {
            let (kind, name) = match node.kind() {
                "decorated_definition" => {
                    let function = named_children(node)
                        .into_iter()
                        .find(|child| child.kind() == "function_definition")
                        .expect("decorated function");
                    let name = function
                        .child_by_field_name("name")
                        .map(|name| node_text(&name, source))
                        .expect("named handler");
                    (DeclKind::Route, name.to_string())
                }
                "function_definition" => {
                    let name = node
                        .child_by_field_name("name")
                        .map(|name| node_text(&name, source))
                        .expect("named function");
                    (DeclKind::Helper, name.to_string())
                }
                _ => continue,
            };
            decls.push(Declaration {
                kind,
                name,
                line: node.start_position().row + 1,
                start_byte: node.start_byte(),
            });
        }
        ParsedModule {
            file: "app.py".to_string(),
            source: source.to_string(),
            tree,
            blueprint: ServiceBlueprint {
                name: "app".to_string(),
                routes: vec![],
                structs: vec![],
                dependencies: vec![],
            },
            decls,
        }
    }

    /// A handler with nine decision points: score 10 against a limit of 8.
    const COMPLEX: &str = r#"
from rivet import api

@api.post("/order")
def create_order(request: dict) -> dict:
    a = request
    if a:
        x = 1
    elif a:
        x = 2
    else:
        x = 3
    if x > 2:
        x = 5
    if x > 3:
        x = 6
    if x > 4:
        x = 7
    for i in range(x):
        x += 1
    while x < 10:
        x += 1
    if x and True:
        return {"n": x}
    return {"n": x}
"#;
    #[test]
    fn handler_above_the_limit_fails_with_e2042_and_an_ast_path() {
        let module = raw_module(COMPLEX);
        let diagnostics = run_gauntlet(&module, &crate::config::GauntletConfig::default());
        assert_eq!(diagnostics.len(), 1);
        let diagnostic = &diagnostics[0];
        assert_eq!(diagnostic.error_code, "E2042");
        assert_eq!(diagnostic.severity, crate::diagnostic::Severity::Blocker);
        assert_eq!(diagnostic.line, Some(4));
        let path = diagnostic.ast_path.as_deref().expect("ast path");
        assert!(path.starts_with("create_order."), "path was {path}");
        assert!(
            !diagnostic.suggested_fix.is_empty(),
            "E2042 must carry a non-empty fix"
        );
    }

    #[test]
    fn simple_handlers_stay_below_the_limit() {
        let module = raw_module(
            r#"
from rivet import api

@api.get("/ping")
def ping() -> dict:
    return {"status": "pong"}
"#,
        );
        let diagnostics = run_gauntlet(&module, &crate::config::GauntletConfig::default());
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn higher_config_limit_lets_the_handler_pass() {
        let module = raw_module(COMPLEX);
        let config = crate::config::GauntletConfig {
            max_complexity: 10,
            ..crate::config::GauntletConfig::default()
        };
        assert!(run_gauntlet(&module, &config).is_empty());
    }
}
