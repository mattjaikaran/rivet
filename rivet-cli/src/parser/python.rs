//! The Python DSL front end.
//!
//! Reads an `app.py`-style module and emits a [`ServiceBlueprint`]. This
//! module orchestrates the pipeline; the grammar work lives in small sibling
//! modules:
//!
//! - [`crate::parser::decorator`]: `api` decorators and route paths
//! - [`crate::parser::signature`]: parameters and return annotations
//! - [`crate::parser::body`]: handler statement lowering
//! - [`crate::parser::expr`]: expression lowering and literal decoding
//! - [`crate::parser::types`]: type annotations and DTO classes
//! - [`crate::parser::validate`]: return/response checking and DTO reachability
//!
//! Supported surface (phase 0):
//!
//! - top-level handlers decorated with `@api.<method>(path, stories=[...])`
//! - handler signatures with a return annotation and at most one
//!   request-body parameter (`dict`, a DTO, or a primitive)
//! - annotation-only DTO classes (`class OrderCreate: sku: str`)
//! - handler bodies that are a single `return` of the supported expression
//!   subset (see [`crate::parser::expr`])
//!
//! Diagnostic codes raised by the front end:
//!
//! | code | meaning |
//! | --- | --- |
//! | E1001 | missing type hint on a parameter or return type |
//! | E1002 | unsupported type annotation syntax |
//! | E1003 | reference to an unknown or non-DTO type |
//! | E1004 | unsupported handler signature (multiple params, defaults, ...) |
//! | E1005 | invalid `api` decorator |
//! | E1006 | unsupported statement or control flow in a handler body |
//! | E1007 | unsupported expression (raised by the expression translator) |
//! | E1008 | file or grammar-level failure |
//! | E1009 | no routes found |
//! | E1010 | return value does not match the declared response type |
//! | E1011 | identifier is not a safe Rust identifier |

use crate::diagnostic::Diagnostic;
use crate::parser::{
    NamedChildren, body, decorator, is_docstring, line_of, node_text, signature, types, validate,
};
use rivet_core::ir::{Expr, ResponseSpec, RouteDefinition, ServiceBlueprint, StructDefinition};
use std::collections::HashMap;
use std::path::Path;
use tree_sitter::{Node, Parser};

/// What a top-level module item is, for the Gauntlet rules.
///
/// Items that the grammar accepts but the engine does not translate
/// (runtime classes, foreign-decorated functions, stray statements) get a
/// declaration so the type-strictness rule can reject them instead of
/// dropping them silently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclKind {
    /// An `api`-decorated function that becomes a route.
    Route,
    /// An undecorated top-level function.
    Helper,
    /// A decorated function whose decorators are not `api.*`.
    Foreign,
    /// An annotation-only class that parses as a DTO.
    Dto,
    /// A class with methods or values, or a decorated class.
    RuntimeClass,
    /// A module-level statement that is neither an import nor a docstring.
    Other,
}

/// One top-level declaration, kept so rules can map a syntax-tree node back
/// to its meaning without re-analyzing the module.
#[derive(Debug, Clone)]
pub struct Declaration {
    pub kind: DeclKind,
    pub name: String,
    /// 1-based source line, for diagnostics.
    pub line: usize,
    /// Byte offset of the node in the source text; nodes and declarations
    /// line up by this key.
    pub start_byte: usize,
}

/// A parsed DSL module: the syntax tree the Gauntlet rules inspect plus the
/// blueprint the generator consumes.
pub struct ParsedModule {
    pub file: String,
    /// The source text, owned so rules can slice it by byte range.
    pub source: String,
    pub tree: tree_sitter::Tree,
    pub blueprint: ServiceBlueprint,
    pub decls: Vec<Declaration>,
}

impl ParsedModule {
    /// The declaration for the top-level node starting at `start_byte`.
    pub fn decl_at(&self, start_byte: usize) -> Option<&Declaration> {
        self.decls.iter().find(|decl| decl.start_byte == start_byte)
    }
}

/// Parse `app.py` from disk into a module the Gauntlet can audit.
pub fn parse_python_file(path: &Path) -> Result<ParsedModule, Diagnostic> {
    let source = std::fs::read_to_string(path).map_err(|err| {
        Diagnostic::blocker("E1008", format!("failed to read {}: {err}", path.display()))
    })?;
    let name = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "app".to_string());
    parse_python_module(&source, &name, &path.display().to_string())
}

/// Parse a Python DSL module from a source string.
///
/// `module_name` becomes the blueprint name (and later the generated package
/// name); `file_label` is used in diagnostics and may be a path or `<source>`.
pub fn parse_python_module(
    source: &str,
    module_name: &str,
    file_label: &str,
) -> Result<ParsedModule, Diagnostic> {
    let mut parser = Parser::new();

    parser
        .set_language(&tree_sitter_python::LANGUAGE.into())
        .map_err(|_| Diagnostic::blocker("E1008", "failed to load the Python grammar"))?;
    let tree = parser
        .parse(source, None)
        .ok_or_else(|| Diagnostic::blocker("E1008", "the parser produced no syntax tree"))?;

    let root = tree.root_node();
    if let Some(error) = first_error(root) {
        return Err(
            Diagnostic::blocker("E1008", "the module contains a Python syntax error").located(
                file_label,
                line_of(&error),
                None::<String>,
            ),
        );
    }

    // Pass zero: classify every top-level item so the Gauntlet rules can
    // find routes, helpers, classes, and stray statements by byte offset.
    let decls = collect_declarations(root, source, file_label)?;

    // Pass one: collect DTO candidate classes in declaration order.
    let mut dtos: HashMap<String, StructDefinition> = HashMap::new();
    let mut dto_order: Vec<String> = Vec::new();
    for child in root.named_children_all() {
        if child.kind() == "class_definition"
            && let Some(dto) = types::parse_dto_class(&child, source, file_label)?
        {
            if dtos.contains_key(&dto.name) {
                return Err(Diagnostic::blocker(
                    "E1003",
                    "duplicate DTO definition; class names must be unique",
                )
                .located(file_label, line_of(&child), None::<String>));
            }
            let name = dto.name.clone();
            dtos.insert(name.clone(), dto);
            dto_order.push(name);
        }
    }

    // Pass two: parse routes. Undecorated functions and non-`api` decorators
    // are not routes and are skipped.
    let mut routes = Vec::new();

    for child in root.named_children_all() {
        if child.kind() == "decorated_definition"
            && let Some(route) = parse_route(&child, source, file_label, &dtos)?
        {
            routes.push(route);
        }
    }

    if routes.is_empty() {
        return Err(Diagnostic::blocker(
            "E1009",
            "no routes found; decorate at least one function with an api decorator, for example @api.get(\"/ping\")",
        )
        .located(file_label, 1, None::<String>));
    }

    // Check DTO references and emit the reachable closure, keeping
    // declaration order so output is stable across runs.
    for route in &routes {
        validate::ensure_known_dtos(route, &dtos, file_label)?;
    }
    let structs = validate::reachable_structs(&routes, &dtos, &dto_order, file_label)?;

    Ok(ParsedModule {
        file: file_label.to_string(),
        source: source.to_string(),
        tree,
        blueprint: ServiceBlueprint {
            name: module_name.to_string(),
            routes,
            structs,
            dependencies: vec![],
        },
        decls,
    })
}

/// Classify the top-level items of a parsed module, in source order.
///
/// Imports, module docstrings, and comments are inert and produce no
/// declaration. Everything else becomes one, so the type-strictness rule
/// can reject module constructs that the engine cannot translate.
fn collect_declarations(
    root: Node<'_>,
    source: &str,
    file: &str,
) -> Result<Vec<Declaration>, Diagnostic> {
    let mut decls = Vec::new();
    for child in root.named_children_all() {
        let start_byte = child.start_byte();
        let line = line_of(&child);
        let declaration = match child.kind() {
            "class_definition" => match types::parse_dto_class(&child, source, file)? {
                Some(dto) => Some(Declaration {
                    kind: DeclKind::Dto,
                    name: dto.name,
                    line,
                    start_byte,
                }),
                None => Some(Declaration {
                    kind: DeclKind::RuntimeClass,
                    name: decl_name(&child, source).to_string(),
                    line,
                    start_byte,
                }),
            },
            "function_definition" => Some(Declaration {
                kind: DeclKind::Helper,
                name: decl_name(&child, source).to_string(),
                line,
                start_byte,
            }),
            "decorated_definition" => classify_decorated(&child, source, file, start_byte, line)?,
            // Imports, comments, and the module docstring are inert.
            "import_statement"
            | "import_from_statement"
            | "future_import_statement"
            | "comment" => None,
            "expression_statement" if is_docstring(&child) => None,
            "expression_statement" => Some(Declaration {
                kind: DeclKind::Other,
                name: "statement".to_string(),
                line,
                start_byte,
            }),
            _ => Some(Declaration {
                kind: DeclKind::Other,
                name: child.kind().to_string(),
                line,
                start_byte,
            }),
        };
        if let Some(declaration) = declaration {
            decls.push(declaration);
        }
    }
    Ok(decls)
}

/// Classify a decorated definition: a route when an `api` decorator is
/// present, otherwise a foreign (untranslatable) definition. Decorated
/// classes count as runtime classes.
fn classify_decorated(
    node: &Node<'_>,
    source: &str,
    file: &str,
    start_byte: usize,
    line: usize,
) -> Result<Option<Declaration>, Diagnostic> {
    let mut api_decorators = 0usize;
    let mut inner_function: Option<String> = None;
    let mut inner_class: Option<String> = None;
    for child in node.named_children_all() {
        match child.kind() {
            "function_definition" => {
                inner_function = Some(decl_name(&child, source).to_string());
            }
            "class_definition" => {
                inner_class = Some(decl_name(&child, source).to_string());
            }
            "decorator" if decorator::parse_api_decorator(&child, source, file)?.is_some() => {
                api_decorators += 1;
            }
            _ => {}
        }
    }
    if let Some(name) = inner_function {
        let kind = if api_decorators > 0 {
            DeclKind::Route
        } else {
            DeclKind::Foreign
        };
        Ok(Some(Declaration {
            kind,
            name,
            line,
            start_byte,
        }))
    } else if let Some(name) = inner_class {
        Ok(Some(Declaration {
            kind: DeclKind::RuntimeClass,
            name,
            line,
            start_byte,
        }))
    } else {
        Ok(None)
    }
}

/// The identifier a class or function definition declares.
fn decl_name<'a>(node: &Node<'_>, source: &'a str) -> &'a str {
    node.child_by_field_name("name")
        .map(|name| node_text(&name, source))
        .unwrap_or("?")
}

/// Parse a route candidate. Returns `None` when the definition carries no
/// `api` decorator (it is an ordinary module function).
fn parse_route(
    node: &Node<'_>,
    source: &str,
    file: &str,
    dtos: &HashMap<String, StructDefinition>,
) -> Result<Option<RouteDefinition>, Diagnostic> {
    let mut function: Option<Node<'_>> = None;
    let mut api_decorators = Vec::new();
    for child in node.named_children_all() {
        match child.kind() {
            "function_definition" => function = Some(child),
            "decorator" => {
                if let Some(metadata) = decorator::parse_api_decorator(&child, source, file)? {
                    api_decorators.push(metadata);
                }
            }
            _ => {}
        }
    }
    if api_decorators.is_empty() {
        return Ok(None);
    }
    let Some(function) = function else {
        return Err(
            Diagnostic::blocker("E1005", "api decorator without a function definition").located(
                file,
                line_of(node),
                None::<String>,
            ),
        );
    };
    if api_decorators.len() > 1 {
        return Err(
            Diagnostic::blocker("E1005", "a handler may carry only one api decorator").located(
                file,
                line_of(&function),
                None::<String>,
            ),
        );
    }
    let Some((method, path, stories)) = api_decorators.pop() else {
        return Err(
            Diagnostic::blocker("E1005", "api decorator metadata is missing").located(
                file,
                line_of(&function),
                None::<String>,
            ),
        );
    };

    let handler_name = function
        .child_by_field_name("name")
        .map(|n| node_text(&n, source))
        .ok_or_else(|| {
            Diagnostic::blocker("E1005", "function without a name").located(
                file,
                line_of(&function),
                None::<String>,
            )
        })?;
    if !crate::parser::is_safe_identifier(handler_name) {
        return Err(Diagnostic::blocker(
            "E1011",
            format!("handler name `{handler_name}` is not a safe Rust identifier"),
        )
        .located(file, line_of(&function), None::<String>));
    }
    decorator::validate_path(&path, file, line_of(&function))?;

    let (request, param_types) = signature::parse_parameters(&function, source, file)?;
    let response = signature::parse_return_type(&function, source, file)?;
    let body_returns = body::parse_handler_body(&function, source, file)?;

    // A bare `return` in a `-> None` handler lowers to JSON null; drop those
    // so the generated handler stays empty. Real values still go through
    // return validation, which rejects them for `-> None`.
    let returns = match &response {
        ResponseSpec::None => body_returns
            .into_iter()
            .filter(|expr| !matches!(expr, Expr::Null))
            .collect(),
        _ => body_returns,
    };
    let route = RouteDefinition {
        method,
        path,
        handler_name: handler_name.to_string(),
        stories,
        middlewares: vec![],
        request,
        response,
        returns,
    };
    validate::validate_returns(&route, file, dtos, &param_types)?;
    Ok(Some(route))
}

/// Find the first ERROR or missing node in a subtree, for syntax-error
/// reporting.
fn first_error(node: Node<'_>) -> Option<Node<'_>> {
    if node.is_error() || node.is_missing() {
        return Some(node);
    }
    for index in 0..node.child_count() {
        let child = node.child(index)?;
        if let Some(error) = first_error(child) {
            return Some(error);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_core::ir::{Expr, HttpMethod, RequestSpec, ServiceBlueprint, TypeRef};

    fn parse(source: &str) -> Result<ServiceBlueprint, Diagnostic> {
        parse_python_module(source, "app", "app.py").map(|module| module.blueprint)
    }

    const ECHO: &str = r#"
from rivet import api

@api.get("/ping")
def ping() -> dict:
    """Health check."""
    return {"status": "pong"}

@api.post("/echo", stories=["US-001"])
def echo(request: dict) -> dict:
    return {"echo": request}
"#;

    #[test]
    fn parses_ping_and_echo() {
        let blueprint = parse(ECHO).expect("parse");
        assert_eq!(blueprint.name, "app");
        assert_eq!(blueprint.routes.len(), 2);
        assert!(blueprint.structs.is_empty());

        let ping = &blueprint.routes[0];
        assert_eq!(ping.method, HttpMethod::Get);
        assert_eq!(ping.path, "/ping");
        assert_eq!(ping.handler_name, "ping");
        assert_eq!(ping.request, RequestSpec::None);
        assert_eq!(ping.response, ResponseSpec::Json(TypeRef::Json));
        assert!(matches!(ping.returns[0], Expr::Object(_)));

        let echo = &blueprint.routes[1];
        assert_eq!(echo.method, HttpMethod::Post);
        assert_eq!(echo.path, "/echo");
        assert_eq!(echo.stories, vec!["US-001"]);
        assert_eq!(
            echo.request,
            RequestSpec::Json {
                var: "request".to_string(),
                ty: TypeRef::Json,
            }
        );
    }

    #[test]
    fn parses_dto_classes_and_constructor_returns() {
        let source = r#"
from rivet import api

class OrderCreate:
    sku: str
    qty: int
    tags: List[str]
    note: Optional[str]

class OrderResponse:
    status: str
    order_id: int

@api.post("/orders", stories=["US-123"])
def create_order(request: OrderCreate) -> OrderResponse:
    return OrderResponse(status="ok", order_id=42)
"#;
        let blueprint = parse(source).expect("parse");
        let names: Vec<&str> = blueprint.structs.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"OrderCreate"));
        assert!(names.contains(&"OrderResponse"));

        let route = &blueprint.routes[0];
        match &route.request {
            RequestSpec::Json { var, ty } => {
                assert_eq!(var, "request");
                assert_eq!(ty, &TypeRef::Named("OrderCreate".to_string()));
            }
            _ => panic!("expected a JSON body"),
        }
        assert!(matches!(route.returns[0], Expr::Construct { .. }));
    }

    #[test]
    fn rejects_missing_return_annotation() {
        let diagnostic =
            parse("from rivet import api\n\n@api.get(\"/ping\")\ndef ping():\n    return {}\n")
                .expect_err("must fail");
        assert_eq!(diagnostic.error_code, "E1001");
        assert_eq!(diagnostic.line, Some(4));
    }

    #[test]
    fn rejects_missing_parameter_annotation() {
        let diagnostic = parse(
            "from rivet import api\n\n@api.post(\"/e\")\ndef e(request) -> dict:\n    return {}\n",
        )
        .expect_err("must fail");
        assert_eq!(diagnostic.error_code, "E1001");
    }

    #[test]
    fn rejects_multiple_parameters() {
        let diagnostic = parse(
            "from rivet import api\n\n@api.post(\"/e\")\ndef e(a: dict, b: dict) -> dict:\n    return {}\n",
        )
        .expect_err("must fail");
        assert_eq!(diagnostic.error_code, "E1004");
    }

    #[test]
    fn rejects_unknown_decorator_keyword() {
        let diagnostic = parse(
            "from rivet import api\n\n@api.get(\"/ping\", rate_limit=10)\ndef ping() -> dict:\n    return {}\n",
        )
        .expect_err("must fail");
        assert_eq!(diagnostic.error_code, "E1005");
    }

    #[test]
    fn rejects_unsupported_body_statement() {
        let diagnostic = parse(
            "from rivet import api\n\n@api.get(\"/ping\")\ndef ping() -> dict:\n    x = 1\n    return {}\n",
        )
        .expect_err("must fail");
        assert_eq!(diagnostic.error_code, "E1006");
    }

    #[test]
    fn rejects_unknown_parameter_reference() {
        let diagnostic = parse(
            "from rivet import api\n\n@api.get(\"/ping\")\ndef ping() -> dict:\n    return {\"x\": nope}\n",
        )
        .expect_err("must fail");
        assert_eq!(diagnostic.error_code, "E1010");
    }

    #[test]
    fn rejects_no_routes() {
        let diagnostic = parse("from rivet import api\n\nx = 1\n").expect_err("must fail");
        assert_eq!(diagnostic.error_code, "E1009");
    }

    #[test]
    fn rejects_unsupported_expression() {
        let diagnostic = parse(
            "from rivet import api\n\n@api.get(\"/ping\")\ndef ping() -> dict:\n    return {\"x\": max(1, 2)}\n",
        )
        .expect_err("must fail");
        assert_eq!(diagnostic.error_code, "E1007");
    }

    #[test]
    fn skips_plain_functions() {
        let blueprint = parse(
            "from rivet import api\n\ndef helper(x):\n    return x\n\n@api.get(\"/ping\")\ndef ping() -> dict:\n    return {}\n",
        )
        .expect("plain functions are skipped");
        assert_eq!(blueprint.routes.len(), 1);
    }

    #[test]
    fn response_none_with_bare_return_is_empty() {
        let blueprint = parse(
            "from rivet import api\n\n@api.delete(\"/thing\")\ndef delete_thing() -> None:\n    return\n",
        )
        .expect("bare return with -> None is fine");
        assert_eq!(blueprint.routes[0].response, ResponseSpec::None);
        assert!(blueprint.routes[0].returns.is_empty());
    }

    #[test]
    fn rejects_value_for_none_response() {
        let diagnostic = parse(
            "from rivet import api\n\n@api.delete(\"/thing\")\ndef delete_thing() -> None:\n    return {\"oops\": True}\n",
        )
        .expect_err("must fail");
        assert_eq!(diagnostic.error_code, "E1010");
    }

    #[test]
    fn parses_fixed_size_array_annotation() {
        let source = r#"
from rivet import api

class Payload:
    embedding: List[float, 768]

@api.post("/vectors")
def vectors(request: Payload) -> dict:
    return {"ok": True}
"#;
        let blueprint = parse(source).expect("parse");
        let payload = blueprint
            .structs
            .iter()
            .find(|s| s.name == "Payload")
            .expect("Payload struct");
        assert_eq!(
            payload.fields[0].type_ref,
            TypeRef::Array {
                element: Box::new(TypeRef::Float),
                len: Some(768),
            }
        );
    }

    #[test]
    fn rejects_unknown_dto_type() {
        let diagnostic = parse(
            "from rivet import api\n\n@api.post(\"/e\")\ndef e(request: Missing) -> dict:\n    return {}\n",
        )
        .expect_err("must fail");
        assert_eq!(diagnostic.error_code, "E1003");
    }

    #[test]
    fn classifies_top_level_declarations() {
        let source = r#"
"""Module docstring."""
from rivet import api

class Order:
    sku: str

class OrderService:
    def create(self) -> None:
        pass

def helper(value: int) -> int:
    return value

@api.post("/orders", stories=["US-1"])
def create_order(request: Order) -> Order:
    return request

@cache
def cached() -> dict:
    return {}

PRICE = 10
"#;
        let module = parse_python_module(source, "app", "app.py").expect("parse");
        let kinds: Vec<(DeclKind, String)> = module
            .decls
            .iter()
            .map(|decl| (decl.kind, decl.name.clone()))
            .collect();
        assert_eq!(
            kinds,
            vec![
                (DeclKind::Dto, "Order".to_string()),
                (DeclKind::RuntimeClass, "OrderService".to_string()),
                (DeclKind::Helper, "helper".to_string()),
                (DeclKind::Route, "create_order".to_string()),
                (DeclKind::Foreign, "cached".to_string()),
                (DeclKind::Other, "statement".to_string()),
            ]
        );
        assert!(
            module
                .decls
                .iter()
                .all(|decl| module.decl_at(decl.start_byte).is_some())
        );
    }

    #[test]
    fn declarations_preserve_source_order_lines() {
        let source = r#"
@api.get("/a")
def a() -> dict:
    return {}

def helper() -> None:
    pass
"#;
        let module = parse_python_module(source, "app", "app.py").expect("parse");
        assert_eq!(module.decls.len(), 2);
        assert_eq!(module.decls[0].kind, DeclKind::Route);
        assert_eq!(module.decls[0].line, 2);
        assert_eq!(module.decls[1].kind, DeclKind::Helper);
        assert_eq!(module.decls[1].line, 6);
    }
}
