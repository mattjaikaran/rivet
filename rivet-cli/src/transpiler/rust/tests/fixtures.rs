//! Blueprints and configs the generator tests share.
//!
//! A sibling test module reaches these through `super::fixtures::*`.

pub(super) use crate::config::RustNativeFeatures;

use super::*;
use rivet_core::ir::{Expr, FieldDefinition, PathParam, RequestSpec, StructDefinition, TypeRef};

pub(super) fn ping_blueprint() -> ServiceBlueprint {
    ServiceBlueprint {
        name: "app".to_string(),
        structs: vec![],
        routes: vec![RouteDefinition {
            method: HttpMethod::Get,
            path: "/ping".to_string(),
            path_params: vec![],
            handler_name: "ping".to_string(),
            stories: vec!["US-001".to_string()],
            middlewares: vec![],
            request: RequestSpec::None,
            response: ResponseSpec::Json(TypeRef::Json),
            returns: vec![Expr::Object(vec![(
                "status".to_string(),
                Expr::Str("pong".to_string()),
            )])],
        }],
        dependencies: vec![],
    }
}

pub(super) fn dto_blueprint() -> ServiceBlueprint {
    ServiceBlueprint {
        name: "app".to_string(),
        structs: vec![StructDefinition {
            name: "OrderResponse".to_string(),
            fields: vec![FieldDefinition {
                name: "status".to_string(),
                type_ref: TypeRef::String,
                is_optional: false,
                is_borrowed: false,
            }],
        }],
        routes: vec![RouteDefinition {
            method: HttpMethod::Post,
            path: "/orders".to_string(),
            path_params: vec![],
            handler_name: "create_order".to_string(),
            stories: vec!["US-123".to_string()],
            middlewares: vec![],
            request: RequestSpec::None,
            response: ResponseSpec::Json(TypeRef::Named("OrderResponse".to_string())),
            returns: vec![Expr::Construct {
                ty: "OrderResponse".to_string(),
                args: vec![("status".to_string(), Expr::Str("ok".to_string()))],
            }],
        }],
        dependencies: vec![],
    }
}

/// A route with one path parameter and no body: `GET /orders/{id}` +
/// `def get_order(id: int) -> dict`.
pub(super) fn path_param_blueprint() -> ServiceBlueprint {
    ServiceBlueprint {
        name: "app".to_string(),
        structs: vec![],
        routes: vec![RouteDefinition {
            method: HttpMethod::Get,
            path: "/orders/{id}".to_string(),
            path_params: vec![PathParam {
                name: "id".to_string(),
                ty: TypeRef::Int,
            }],
            handler_name: "get_order".to_string(),
            stories: vec!["US-010".to_string()],
            middlewares: vec![],
            request: RequestSpec::None,
            response: ResponseSpec::Json(TypeRef::Json),
            returns: vec![Expr::Object(vec![(
                "id".to_string(),
                Expr::Ident("id".to_string()),
            )])],
        }],
        dependencies: vec![],
    }
}

/// A route with one path parameter and a JSON body: `PUT /orders/{id}` +
/// `def update_order(id: int, request: str) -> dict`.
pub(super) fn path_param_body_blueprint() -> ServiceBlueprint {
    ServiceBlueprint {
        name: "app".to_string(),
        structs: vec![],
        routes: vec![RouteDefinition {
            method: HttpMethod::Put,
            path: "/orders/{id}".to_string(),
            path_params: vec![PathParam {
                name: "id".to_string(),
                ty: TypeRef::Int,
            }],
            handler_name: "update_order".to_string(),
            stories: vec!["US-011".to_string()],
            middlewares: vec![],
            request: RequestSpec::Json {
                var: "request".to_string(),
                ty: TypeRef::String,
            },
            response: ResponseSpec::Json(TypeRef::Json),
            returns: vec![Expr::Object(vec![
                ("id".to_string(), Expr::Ident("id".to_string())),
                ("name".to_string(), Expr::Ident("request".to_string())),
            ])],
        }],
        dependencies: vec![],
    }
}

/// A route with two path parameters and no body:
/// `GET /users/{user_id}/orders/{id}` +
/// `def get_user_order(user_id: str, id: int) -> dict`.
pub(super) fn two_path_params_blueprint() -> ServiceBlueprint {
    ServiceBlueprint {
        name: "app".to_string(),
        structs: vec![],
        routes: vec![RouteDefinition {
            method: HttpMethod::Get,
            path: "/users/{user_id}/orders/{id}".to_string(),
            path_params: vec![
                PathParam {
                    name: "user_id".to_string(),
                    ty: TypeRef::String,
                },
                PathParam {
                    name: "id".to_string(),
                    ty: TypeRef::Int,
                },
            ],
            handler_name: "get_user_order".to_string(),
            stories: vec!["US-012".to_string()],
            middlewares: vec![],
            request: RequestSpec::None,
            response: ResponseSpec::Json(TypeRef::Json),
            returns: vec![Expr::Object(vec![
                ("user_id".to_string(), Expr::Ident("user_id".to_string())),
                ("id".to_string(), Expr::Ident("id".to_string())),
            ])],
        }],
        dependencies: vec![],
    }
}

pub(super) fn grpc_config() -> RivetConfig {
    RivetConfig {
        transport: Transport {
            mode: TransportMode::Grpc,
            grpc_port: 51000,
        },
        ..RivetConfig::default()
    }
}
