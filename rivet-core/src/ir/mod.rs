//! The intermediate representation (IR).
//!
//! Every language front end (Python DSL today, TypeScript DSL later) must
//! produce a [`ServiceBlueprint`]. Every code generator consumes one. The IR
//! is plain data: it serializes with serde, so tooling, tests, and the
//! context engine can persist and compare blueprints.
//!
//! Phase 0 subset: one module (file) per blueprint, handlers with an optional
//! single JSON-body parameter, and a return body built from literals,
//! request parameters, or one DTO constructor call. Anything outside that
//! subset fails with a structured diagnostic instead of a wrong translation.

mod expr;

pub use expr::{BinOp, Expr, Stmt, binary_result};

use serde::{Deserialize, Serialize};

/// The HTTP verb a route responds to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Delete,
    Patch,
    Options,
    Head,
}

impl HttpMethod {
    /// Wire name, for example `GET`.
    pub fn as_str(self) -> &'static str {
        match self {
            HttpMethod::Get => "GET",
            HttpMethod::Post => "POST",
            HttpMethod::Put => "PUT",
            HttpMethod::Delete => "DELETE",
            HttpMethod::Patch => "PATCH",
            HttpMethod::Options => "OPTIONS",
            HttpMethod::Head => "HEAD",
        }
    }

    /// axum routing function that serves this method, for example `get`.
    pub fn axum_router_fn(self) -> &'static str {
        match self {
            HttpMethod::Get => "get",
            HttpMethod::Post => "post",
            HttpMethod::Put => "put",
            HttpMethod::Delete => "delete",
            HttpMethod::Patch => "patch",
            HttpMethod::Options => "options",
            HttpMethod::Head => "head",
        }
    }
}

/// A type reference resolved from a DSL annotation.
///
/// Field [`is_optional`](FieldDefinition::is_optional) and
/// [`is_borrowed`](FieldDefinition::is_borrowed) are tracked separately
/// because they change how a field renders (wrapping in `Option<T>`, or
/// borrowing with a lifetime instead of owning).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TypeRef {
    /// `str`
    String,
    /// `bool`
    Bool,
    /// `int`
    Int,
    /// `float`
    Float,
    /// `dict` — a free-form JSON value.
    Json,
    /// `list[T]`, or `list[T, N]` for a fixed-size array when `len` is set.
    Array {
        element: Box<TypeRef>,
        len: Option<usize>,
    },
    /// A DTO by name; resolve through [`ServiceBlueprint::structs`].
    Named(String),
}

/// One field of a DSL DTO class.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldDefinition {
    pub name: String,
    pub type_ref: TypeRef,
    pub is_optional: bool,
    #[serde(default)]
    pub is_borrowed: bool,
}

/// A DTO declared in the DSL (an annotation-only class).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructDefinition {
    pub name: String,
    pub fields: Vec<FieldDefinition>,
}

impl TypeRef {
    /// A human label for the type, as the DSL writes it.
    ///
    /// Both the parser's diagnostics and the generator's use the same label,
    /// so a message about a type reads the same wherever it comes from.
    pub fn label(&self) -> String {
        match self {
            TypeRef::String => "str".to_string(),
            TypeRef::Bool => "bool".to_string(),
            TypeRef::Int => "int".to_string(),
            TypeRef::Float => "float".to_string(),
            TypeRef::Json => "dict".to_string(),
            TypeRef::Array { .. } => "list".to_string(),
            TypeRef::Named(name) => name.clone(),
        }
    }
}
/// A parameter a route reads from somewhere other than its response body.
///
/// One type serves both sources, because a path placeholder and a query
/// parameter differ only in where the value comes from. The order of a
/// [`RouteDefinition::path_params`] list matches the placeholders in
/// [`RouteDefinition::path`], because the generated router binds them
/// positionally; a `query_params` list keeps the order the handler declares.
/// The parser resolves each type from the handler signature and rejects a
/// value the handler does not declare, so every entry has a matching
/// parameter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteParam {
    /// The name, without the braces for a path placeholder.
    pub name: String,
    /// The type the handler declares for it.
    pub ty: TypeRef,
}

/// How a route receives its request body. A path or query value is not part
/// of this: those live in [`RouteDefinition::path_params`] and
/// [`RouteDefinition::query_params`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RequestSpec {
    /// No request body (for example a bare `GET`).
    None,
    /// A JSON body deserialized into `ty`, bound to handler parameter `var`.
    Json { var: String, ty: TypeRef },
}

/// What a route returns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResponseSpec {
    /// No response body (`-> None`).
    None,
    /// A JSON response of the given type (`-> dict`, `-> int`, ...).
    Json(TypeRef),
}

/// A single route: DSL decorator plus handler signature and return body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouteDefinition {
    pub method: HttpMethod,
    pub path: String,
    /// `{name}` placeholders in `path`, in path order, with the type the
    /// handler declares for each. Empty for a path with no placeholders.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path_params: Vec<RouteParam>,
    /// Query-string parameters, in declaration order, with the type the
    /// handler declares for each. Empty for a route that reads no query.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub query_params: Vec<RouteParam>,
    /// Rust-safe handler name, identical to the DSL function name.
    pub handler_name: String,
    /// User-story IDs; the Verifier requires at least one per endpoint.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stories: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub middlewares: Vec<String>,
    pub request: RequestSpec,
    pub response: ResponseSpec,
    /// The handler body, in source order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub body: Vec<Stmt>,
}

/// The parse result for one DSL entry point (`app.py`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServiceBlueprint {
    pub name: String,
    pub routes: Vec<RouteDefinition>,
    /// DTO definitions shared by routes (request and response bodies).
    #[serde(default)]
    pub structs: Vec<StructDefinition>,
    /// Services this blueprint depends on; empty until the multi-service phase.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ServiceBlueprint {
        ServiceBlueprint {
            name: "app".to_string(),
            structs: vec![StructDefinition {
                name: "OrderCreate".to_string(),
                fields: vec![
                    FieldDefinition {
                        name: "customer".to_string(),
                        type_ref: TypeRef::String,
                        is_optional: false,
                        is_borrowed: false,
                    },
                    FieldDefinition {
                        name: "sku".to_string(),
                        type_ref: TypeRef::Array {
                            element: Box::new(TypeRef::String),
                            len: None,
                        },
                        is_optional: true,
                        is_borrowed: false,
                    },
                ],
            }],
            routes: vec![RouteDefinition {
                method: HttpMethod::Post,
                path: "/orders".to_string(),
                path_params: vec![],
                handler_name: "create_order".to_string(),
                stories: vec!["US-123".to_string()],
                middlewares: vec![],
                request: RequestSpec::Json {
                    var: "request".to_string(),
                    ty: TypeRef::Named("OrderCreate".to_string()),
                },
                query_params: vec![],
                response: ResponseSpec::Json(TypeRef::Named("OrderResponse".to_string())),
                body: vec![],
            }],
            dependencies: vec![],
        }
    }

    #[test]
    fn http_method_metadata_is_consistent() {
        let pairs = [
            (HttpMethod::Get, "GET", "get"),
            (HttpMethod::Post, "POST", "post"),
            (HttpMethod::Put, "PUT", "put"),
            (HttpMethod::Delete, "DELETE", "delete"),
            (HttpMethod::Patch, "PATCH", "patch"),
            (HttpMethod::Options, "OPTIONS", "options"),
            (HttpMethod::Head, "HEAD", "head"),
        ];
        for (method, wire, router) in pairs {
            assert_eq!(method.as_str(), wire);
            assert_eq!(method.axum_router_fn(), router);
        }
    }

    #[test]
    fn blueprint_round_trips_through_json() {
        let blueprint = sample();
        let json = serde_json::to_string(&blueprint).expect("serialize blueprint");
        let back: ServiceBlueprint = serde_json::from_str(&json).expect("deserialize blueprint");
        assert_eq!(blueprint, back);
    }

    #[test]
    fn defaulted_fields_serialize_compactly() {
        // defaults keep IR dumps readable: no empty vecs sprayed everywhere
        let json = serde_json::to_string(&sample()).expect("serialize blueprint");
        assert!(!json.contains("\"stories\":[]"));
        assert!(!json.contains("\"body\":[]"));
    }

    /// The binary rule is the only place an operand pair becomes a type, so
    #[test]
    fn division_is_always_true_division() {
        use TypeRef::{Float, Int, String};
        assert_eq!(binary_result(BinOp::Div, &Int, &Int), Some(Float));
        assert_eq!(binary_result(BinOp::Add, &Int, &Float), Some(Float));
        assert_eq!(binary_result(BinOp::Add, &String, &String), Some(String));
        assert_eq!(binary_result(BinOp::Mul, &String, &Int), None);
        assert_eq!(binary_result(BinOp::Add, &TypeRef::Bool, &Int), None);
    }

    #[test]
    fn a_comparison_yields_a_bool_and_needs_comparable_operands() {
        use TypeRef::{Bool, Float, Int, String};
        assert_eq!(binary_result(BinOp::Lt, &Int, &Float), Some(Bool));
        assert_eq!(binary_result(BinOp::Eq, &String, &String), Some(Bool));
        assert_eq!(binary_result(BinOp::Gt, &Int, &String), None);
        assert_eq!(binary_result(BinOp::And, &Bool, &Bool), Some(Bool));
        assert_eq!(binary_result(BinOp::And, &Int, &Bool), None);
    }

    /// A body returns on every path only when the `else` branch exists and
    /// every branch returns; otherwise the generated function falls off its
    /// end and the crate does not compile.
    #[test]
    fn all_paths_return_needs_an_else_and_a_return_in_each_branch() {
        let value = Expr::Int(1);
        let returns = vec![Stmt::Return(value.clone())];
        let empty = vec![];
        assert!(Stmt::all_paths_return(&returns));
        assert!(!Stmt::all_paths_return(&empty));

        let with_else = vec![Stmt::If {
            branches: vec![(Expr::Bool(true), returns.clone())],
            otherwise: returns.clone(),
        }];
        assert!(Stmt::all_paths_return(&with_else));

        let without_else = vec![Stmt::If {
            branches: vec![(Expr::Bool(true), returns.clone())],
            otherwise: empty,
        }];
        assert!(!Stmt::all_paths_return(&without_else));
    }

    /// A `for` may run zero times, so it never completes a handler; a `match`
    /// completes only through a wildcard arm whose body completes.
    #[test]
    fn a_loop_never_completes_and_a_match_completes_through_its_wildcard() {
        let returns = vec![Stmt::Return(Expr::Int(1))];
        let arms = |wildcard: bool| {
            let mut arms = vec![(Some(Expr::Int(1)), returns.clone())];
            if wildcard {
                arms.push((None, returns.clone()));
            }
            arms
        };
        let match_on = |arms: Vec<(Option<Expr>, Vec<Stmt>)>| {
            vec![Stmt::Match {
                subject: Expr::Ident("score".to_string()),
                arms,
            }]
        };

        assert!(!Stmt::all_paths_return(&[Stmt::For {
            name: "line".to_string(),
            ty: TypeRef::Int,
            iterable: Expr::Array(vec![Expr::Int(1)]),
            body: returns.clone(),
        }]));
        assert!(Stmt::all_paths_return(&match_on(arms(true))));
        assert!(!Stmt::all_paths_return(&match_on(arms(false))));
        assert!(!Stmt::all_paths_return(&match_on(vec![
            (Some(Expr::Int(1)), returns.clone()),
            (None, vec![]),
        ])));
    }
}
