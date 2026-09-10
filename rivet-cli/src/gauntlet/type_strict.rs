//! Type-strictness rule (`E2046`): the DSL module must translate whole.
//!
//! The parser lowers routes and DTOs to the IR and *skips* everything else.
//! Skipped Python silently vanishes from the generated server — the source
//! says one thing and the binary does another. This rule closes that gap by
//! rejecting module constructs that have no IR home.
//!
//! Accepted module surface (the documented contract):
//!
//! - a module docstring and `import` statements (inert)
//! - `api`-decorated handler functions (routes)
//! - undecorated helper functions
//! - annotation-only DTO classes
//!
//! Dynamic typing is allowed only at the JSON boundary, where the IR models
//! it explicitly:
//!
//! - `dict` request bodies and `-> dict` responses
//! - DTO fields of type `dict`, `list`, `List[dict]`, or `Optional[...]`
//!   of those
//! - JSON literals, request parameters, and containers in handler bodies
//!   under such types
//!
//! Rejected with `E2046`:
//!
//! - classes with methods or runtime values (runtime classes)
//! - decorated functions without an `api` decorator (foreign decorators)
//! - non-`api` decorators stacked on a handler (the generator would drop
//!   them silently)
//! - module-level statements other than imports and the docstring
//!
//! Anything the parser cannot type already fails earlier with an `E1xxx`
//! diagnostic; this rule owns the module-level contract.

use crate::gauntlet::{Context, Finding, Rule};
use crate::parser::NamedChildren;
use crate::parser::decorator::parse_api_decorator;
use crate::parser::python::DeclKind;
use tree_sitter::Node;

pub(crate) struct TypeStrict;

/// Non-`api` decorators on a decorated definition. A decorator that fails
/// to parse is not counted: the parser rejects it before the Gauntlet runs.
fn foreign_decorator_count(node: &Node<'_>, source: &str, file: &str) -> usize {
    node.named_children_all()
        .into_iter()
        .filter(|child| {
            child.kind() == "decorator"
                && matches!(parse_api_decorator(child, source, file), Ok(None))
        })
        .count()
}

impl Rule for TypeStrict {
    fn id(&self) -> &'static str {
        "type_strictness"
    }

    fn default_severity(&self) -> crate::diagnostic::Severity {
        crate::diagnostic::Severity::Blocker
    }

    fn check(&self, ctx: &Context<'_>) -> Vec<Finding> {
        let root = ctx.module.tree.root_node();
        let mut findings = Vec::new();
        for node in root.named_children_all() {
            let Some(decl) = ctx.module.decl_at(node.start_byte()) else {
                continue;
            };
            let finding = match decl.kind {
                DeclKind::RuntimeClass => Some(
                    Finding::new(
                        "E2046",
                        ctx.severity,
                        format!(
                            "class `{}` is not an annotation-only DTO; its code would not reach the generated server",
                            decl.name
                        ),
                        &ctx.module.file,
                        decl.line,
                    )
                    .at_path(decl.name.clone())
                    .with_fix("convert the class to annotation-only DTO fields, or remove it"),
                ),
                DeclKind::Foreign => Some(
                    Finding::new(
                        "E2046",
                        ctx.severity,
                        format!(
                            "function `{}` is decorated but not with an api decorator; the decorator would not reach the generated server",
                            decl.name
                        ),
                        &ctx.module.file,
                        decl.line,
                    )
                    .at_path(decl.name.clone())
                    .with_fix("remove the decorator, or give the function an api decorator"),
                ),
                DeclKind::Route => {
                    let foreign =
                        foreign_decorator_count(&node, &ctx.module.source, &ctx.module.file);
                    if foreign == 0 {
                        None
                    } else {
                        Some(
                            Finding::new(
                                "E2046",
                                ctx.severity,
                                format!(
                                    "handler `{}` carries {foreign} non-api decorator{} that the generator would drop silently",
                                    decl.name,
                                    if foreign == 1 { "" } else { "s" }
                                ),
                                &ctx.module.file,
                                decl.line,
                            )
                            .at_path(decl.name.clone())
                            .with_fix("remove the decorator, or move its behavior into the handler"),
                        )
                    }
                }
                DeclKind::Other => Some(
                    Finding::new(
                        "E2046",
                        ctx.severity,
                        format!(
                            "module-level {} statements do not transpile; app.py may hold routes, helpers, DTO classes, imports, and a docstring",
                            decl.name
                        ),
                        &ctx.module.file,
                        decl.line,
                    )
                    .at_path(format!("module.{}", decl.name))
                    .with_fix("remove the statement, or move it into a helper function"),
                ),
                DeclKind::Helper | DeclKind::Dto => None,
            };
            if let Some(finding) = finding {
                findings.push(finding);
            }
        }
        findings
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::GauntletConfig;
    use crate::gauntlet::run_rule;
    use crate::parser::python::parse_python_module;

    fn module(source: &str) -> crate::parser::python::ParsedModule {
        parse_python_module(source, "app", "app.py").expect("module must parse")
    }

    fn findings(source: &str) -> Vec<crate::diagnostic::Diagnostic> {
        let parsed = module(source);
        run_rule(&parsed, &GauntletConfig::default(), &TypeStrict)
    }

    #[test]
    fn runtime_class_is_rejected() {
        let diagnostics = findings(
            r#"
from rivet import api

class OrderService:
    def create(self) -> None:
        pass

@api.get("/ping", stories=["US-1"])
def ping() -> dict:
    return {"status": "pong"}
"#,
        );
        assert_eq!(diagnostics.len(), 1);
        let diagnostic = &diagnostics[0];
        assert_eq!(diagnostic.error_code, "E2046");
        assert_eq!(diagnostic.line, Some(4));
        assert!(diagnostic.message.contains("OrderService"));
        assert!(
            !diagnostic.suggested_fix.is_empty(),
            "E2046 must carry a non-empty fix"
        );
    }

    #[test]
    fn foreign_decorator_is_rejected() {
        let diagnostics = findings(
            r#"
from rivet import api

@cache
def cached() -> dict:
    return {}

@api.get("/ping", stories=["US-1"])
def ping() -> dict:
    return {"status": "pong"}
"#,
        );
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("cached"));
        assert!(!diagnostics[0].suggested_fix.is_empty());
    }

    #[test]
    fn extra_non_api_decorator_on_a_route_is_rejected() {
        let diagnostics = findings(
            r#"
from rivet import api

@api.get("/ping", stories=["US-1"])
@auth.admin
def ping() -> dict:
    return {"status": "pong"}
"#,
        );
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("non-api decorator"));
        assert!(diagnostics[0].message.contains("ping"));
        assert!(!diagnostics[0].suggested_fix.is_empty());
    }

    #[test]
    fn module_level_statement_is_rejected() {
        let diagnostics = findings(
            r#"
from rivet import api

PRICE = 10

@api.get("/ping", stories=["US-1"])
def ping() -> dict:
    return {"status": "pong"}
"#,
        );
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("statement"));
        assert!(!diagnostics[0].suggested_fix.is_empty());
    }

    #[test]
    fn documented_json_shapes_pass() {
        // dict bodies and fields, list DTO fields, Optional wrappers, and
        // the plain module docstring all sit inside the accepted contract.
        let diagnostics = findings(
            r#"
"""A conforming module."""
from rivet import api

class LineItem:
    sku: str
    meta: dict
    tags: list
    aliases: Optional[List[str]]

@api.post("/echo", stories=["US-1"])
def echo(request: dict) -> dict:
    return {"echo": request}
"#,
        );
        assert!(diagnostics.is_empty());
    }
}
