//! Blueprints and configs the generator tests share.
//!
//! A sibling test module reaches these through `super::fixtures::*`.

pub(super) use crate::config::RustNativeFeatures;

use super::*;
use rivet_core::ir::{Expr, FieldDefinition, RequestSpec, StructDefinition, TypeRef};

pub(super) fn ping_blueprint() -> ServiceBlueprint {
    ServiceBlueprint {
        name: "app".to_string(),
        structs: vec![],
        routes: vec![RouteDefinition {
            method: HttpMethod::Get,
            path: "/ping".to_string(),
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

pub(super) fn grpc_config() -> RivetConfig {
    RivetConfig {
        transport: Transport {
            mode: TransportMode::Grpc,
            grpc_port: 51000,
        },
        ..RivetConfig::default()
    }
}
