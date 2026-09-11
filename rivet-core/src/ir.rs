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

/// One `{name}` placeholder in a route path, with the type its handler
/// declares for it.
///
/// The order matches the placeholders in [`RouteDefinition::path`], because
/// the generated router binds them positionally. The parser resolves the type
/// from the handler signature and rejects a placeholder the handler does not
/// declare, so every entry here has a matching parameter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathParam {
    /// The placeholder name, without the braces.
    pub name: String,
    /// The type the handler declares for it.
    pub ty: TypeRef,
}

/// How a route receives its request. Phase 0 supports an optional JSON body.
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

/// A handler return expression, translated from the DSL.
///
/// Values are stored typed so generators can render each one into the target
/// language without re-analyzing the source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Expr {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    /// `[a, b]` — an array literal.
    Array(Vec<Expr>),
    /// `{"key": value}` — an object literal with string keys.
    Object(Vec<(String, Expr)>),
    /// A reference to a handler parameter by name.
    Ident(String),
    /// A DTO constructor call such as `OrderResponse(status="ok")`.
    Construct {
        ty: String,
        args: Vec<(String, Expr)>,
    },
}

/// A single route: DSL decorator plus handler signature and return body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouteDefinition {
    pub method: HttpMethod,
    pub path: String,
    /// `{name}` placeholders in `path`, in path order, with the type the
    /// handler declares for each. Empty for a path with no placeholders.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path_params: Vec<PathParam>,
    /// Rust-safe handler name, identical to the DSL function name.
    pub handler_name: String,
    /// User-story IDs; the Gauntlet requires at least one per endpoint.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stories: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub middlewares: Vec<String>,
    pub request: RequestSpec,
    pub response: ResponseSpec,
    /// Return statements. Phase 0 permits at most one, and the parser enforces
    /// it; the type is a vector so later phases can model control flow.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub returns: Vec<Expr>,
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
                response: ResponseSpec::Json(TypeRef::Named("OrderResponse".to_string())),
                returns: vec![],
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
        assert!(!json.contains("\"returns\":[]"));
    }
}
