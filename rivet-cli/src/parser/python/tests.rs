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
    assert!(!diagnostic.suggested_fix.is_empty());
}

#[test]
fn rejects_missing_parameter_annotation() {
    let diagnostic = parse(
        "from rivet import api\n\n@api.post(\"/e\")\ndef e(request) -> dict:\n    return {}\n",
    )
    .expect_err("must fail");
    assert_eq!(diagnostic.error_code, "E1001");
    assert!(!diagnostic.suggested_fix.is_empty());
}

#[test]
fn rejects_multiple_parameters() {
    let diagnostic = parse(
        "from rivet import api\n\n@api.post(\"/e\")\ndef e(a: dict, b: dict) -> dict:\n    return {}\n",
    )
    .expect_err("must fail");
    assert_eq!(diagnostic.error_code, "E1004");
    assert!(!diagnostic.suggested_fix.is_empty());
}

#[test]
fn rejects_unknown_decorator_keyword() {
    let diagnostic = parse(
        "from rivet import api\n\n@api.get(\"/ping\", rate_limit=10)\ndef ping() -> dict:\n    return {}\n",
    )
    .expect_err("must fail");
    assert_eq!(diagnostic.error_code, "E1005");
    assert!(!diagnostic.suggested_fix.is_empty());
}

#[test]
fn rejects_unsupported_body_statement() {
    let diagnostic = parse(
        "from rivet import api\n\n@api.get(\"/ping\")\ndef ping() -> dict:\n    x = 1\n    return {}\n",
    )
    .expect_err("must fail");
    assert_eq!(diagnostic.error_code, "E1006");
    assert!(!diagnostic.suggested_fix.is_empty());
}

#[test]
fn rejects_unknown_parameter_reference() {
    let diagnostic = parse(
        "from rivet import api\n\n@api.get(\"/ping\")\ndef ping() -> dict:\n    return {\"x\": nope}\n",
    )
    .expect_err("must fail");
    assert_eq!(diagnostic.error_code, "E1010");
    assert!(!diagnostic.suggested_fix.is_empty());
}

#[test]
fn rejects_no_routes() {
    let diagnostic = parse("from rivet import api\n\nx = 1\n").expect_err("must fail");
    assert_eq!(diagnostic.error_code, "E1009");
    assert!(!diagnostic.suggested_fix.is_empty());
}

#[test]
fn rejects_unsupported_expression() {
    let diagnostic = parse(
        "from rivet import api\n\n@api.get(\"/ping\")\ndef ping() -> dict:\n    return {\"x\": max(1, 2)}\n",
    )
    .expect_err("must fail");
    assert_eq!(diagnostic.error_code, "E1007");
    assert!(!diagnostic.suggested_fix.is_empty());
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
    assert!(!diagnostic.suggested_fix.is_empty());
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
    assert!(!diagnostic.suggested_fix.is_empty());
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

#[test]
fn two_routes_cannot_share_a_handler_name() {
    // Python accepts this module: the second `def` rebinds the name. The
    // front end rejects it, because the handler name becomes a Rust
    // function name and the generated crate would define it twice.
    let source = "@api.get(\"/ping\")\ndef ping() -> dict:\n    return {}\n\n@api.get(\"/health\")\ndef ping() -> dict:\n    return {}\n";
    let error = match parse_python_module(source, "app", "app.py") {
        Ok(_) => panic!("a duplicate handler must not parse"),
        Err(error) => error,
    };
    assert_eq!(error.error_code, "E1012");
    assert!(error.message.contains("ping"), "{}", error.message);
    // The decorated definition starts at its decorator, which is what makes
    // the function a route.
    assert_eq!(error.line, Some(5), "the second route is reported");
    assert!(!error.suggested_fix.is_empty());
}

#[test]
fn distinct_handler_names_on_one_shape_still_parse() {
    let source = "@api.get(\"/ping\")\ndef ping() -> dict:\n    return {}\n\n@api.get(\"/health\")\ndef health() -> dict:\n    return {}\n";
    let module = parse_python_module(source, "app", "app.py").expect("parse");
    assert_eq!(module.blueprint.routes.len(), 2);
}

/// A DTO that reuses a name the generated crate holds is rejected with
/// `E1013`, at a file and a line.
#[test]
fn a_dto_name_the_generated_crate_owns_is_rejected() {
    for name in ["String", "State", "Router", "service", "serde_json", "main"] {
        let source = format!(
            "from rivet import api\n\nclass {name}:\n    value: str\n\n@api.post(\"/s\", stories=[\"US-1\"])\ndef put_s(request: {name}) -> {name}:\n    return request\n"
        );
        let error = match parse_python_module(&source, "app", "app.py") {
            Ok(_) => panic!("DTO `{name}` collides with generated code and must not parse"),
            Err(error) => error,
        };
        assert_eq!(error.error_code, "E1013", "DTO `{name}`: {}", error.message);
        assert!(
            error.message.contains(name),
            "the diagnostic names the identifier: {}",
            error.message
        );
        assert_eq!(error.file.as_deref(), Some("app.py"));
        assert_eq!(error.line, Some(3), "the class declaration is reported");
        assert!(!error.suggested_fix.is_empty());
    }
}

/// A DTO whose name merely contains a reserved one is accepted: the check is
/// an equality against a set, not a substring match.
#[test]
fn a_dto_name_that_only_resembles_a_reserved_one_parses() {
    let source = "from rivet import api\n\nclass Stringify:\n    value: str\n\n@api.post(\"/s\", stories=[\"US-1\"])\ndef stringify(request: Stringify) -> Stringify:\n    return request\n";
    let blueprint = parse(source).expect("`Stringify` is not a reserved name");
    assert_eq!(blueprint.structs[0].name, "Stringify");
}

/// A handler that carries the reserved prefix is rejected with `E1013`.
#[test]
fn a_handler_name_with_the_reserved_prefix_is_rejected() {
    let source = "from rivet import api\n\n@api.get(\"/ping\", stories=[\"US-1\"])\ndef rivet_ping() -> dict:\n    return {}\n";
    let error = match parse_python_module(source, "app", "app.py") {
        Ok(_) => panic!("a handler with the reserved prefix must not parse"),
        Err(error) => error,
    };
    assert_eq!(error.error_code, "E1013");
    assert!(error.message.contains("rivet_ping"), "{}", error.message);
    assert_eq!(error.line, Some(4));
}

/// A handler may reuse a reserved *type* name.
///
/// The probe decides this row, and it decides for the namespace, not for
/// taste: the native generator emits handlers inside `mod handlers`, where a
/// local function shadows the crate root's glob import without error, so
/// `def service()` builds. A DTO takes the full reserved set because a DTO is
/// a crate-root struct; a handler takes the prefix alone.
#[test]
fn a_handler_name_the_generated_crate_owns_still_parses() {
    for name in ["service", "String", "State", "Router", "main"] {
        let source = format!(
            "from rivet import api\n\n@api.get(\"/m\", stories=[\"US-1\"])\ndef {name}() -> dict:\n    return {{}}\n"
        );
        let module = parse_python_module(&source, "app", "app.py")
            .unwrap_or_else(|error| panic!("handler `{name}` is legal: {}", error.message));
        assert_eq!(module.blueprint.routes[0].handler_name, name);
    }
}
