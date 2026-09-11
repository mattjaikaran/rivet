//! The borrowed-DTO rules: the opt-in and the legal positions.

use super::*;
use crate::config::{RivetConfig, RustNativeFeatures};
use rivet_core::ir::{
    Expr, FieldDefinition, HttpMethod, RequestSpec, ResponseSpec, RouteDefinition, TypeRef,
};

/// A DTO whose text borrows from the request body.
fn note() -> StructDefinition {
    StructDefinition {
        name: "Note".to_string(),
        fields: vec![FieldDefinition {
            name: "text".to_string(),
            type_ref: TypeRef::String,
            is_optional: false,
            is_borrowed: true,
        }],
    }
}

/// An owned DTO, for a route whose request does not borrow.
fn ack() -> StructDefinition {
    StructDefinition {
        name: "Ack".to_string(),
        fields: vec![FieldDefinition {
            name: "status".to_string(),
            type_ref: TypeRef::String,
            is_optional: false,
            is_borrowed: false,
        }],
    }
}

/// A route taking `request_ty` and replying with `response_ty`.
fn route(request_ty: TypeRef, response_ty: TypeRef) -> RouteDefinition {
    RouteDefinition {
        method: HttpMethod::Post,
        path: "/notes".to_string(),
        path_params: vec![],
        query_params: vec![],
        handler_name: "create".to_string(),
        stories: vec!["US-004".to_string()],
        middlewares: vec![],
        request: RequestSpec::Json {
            var: "request".to_string(),
            ty: request_ty,
        },
        response: ResponseSpec::Json(response_ty),
        returns: vec![Expr::Ident("request".to_string())],
    }
}

fn blueprint(structs: Vec<StructDefinition>, routes: Vec<RouteDefinition>) -> ServiceBlueprint {
    ServiceBlueprint {
        name: "app".to_string(),
        structs,
        routes,
        dependencies: vec![],
    }
}

fn borrowed_config() -> RivetConfig {
    RivetConfig {
        rust_native_features: RustNativeFeatures {
            zero_copy_deserialization: true,
            ..RustNativeFeatures::default()
        },
        ..RivetConfig::default()
    }
}

/// The borrowed DTO as a request body, with an owned reply.
fn legal_blueprint() -> ServiceBlueprint {
    blueprint(
        vec![note(), ack()],
        vec![route(
            TypeRef::Named("Note".to_string()),
            TypeRef::Named("Ack".to_string()),
        )],
    )
}

#[test]
fn the_flag_gates_a_borrowed_field() {
    let error = check(&legal_blueprint(), &RivetConfig::default()).expect_err("the flag is off");
    assert_eq!(error.error_code, "E2015");
    assert!(
        error.message.contains("zero_copy_deserialization"),
        "the message names the flag: {}",
        error.message
    );
    assert!(
        error
            .suggested_fix
            .contains("zero_copy_deserialization = true"),
        "the fix names the flag: {}",
        error.suggested_fix
    );
}

#[test]
fn a_borrowed_request_body_is_legal_under_the_flag() {
    check(&legal_blueprint(), &borrowed_config()).expect("a request body is the borrowed position");
}

#[test]
fn an_owned_dto_needs_no_opt_in() {
    let owned = blueprint(
        vec![ack()],
        vec![route(
            TypeRef::Named("Ack".to_string()),
            TypeRef::Named("Ack".to_string()),
        )],
    );
    check(&owned, &RivetConfig::default()).expect("an owned DTO is unaffected");
}

#[test]
fn a_borrowed_response_type_is_rejected() {
    let borrowed_reply = blueprint(
        vec![note(), ack()],
        vec![route(
            TypeRef::Named("Ack".to_string()),
            TypeRef::Named("Note".to_string()),
        )],
    );
    let error = check(&borrowed_reply, &borrowed_config()).expect_err("a reply cannot borrow");
    assert_eq!(error.error_code, "E2016");
    assert!(
        error.message.contains("response type"),
        "the message names the position: {}",
        error.message
    );
}

#[test]
fn an_array_of_a_borrowed_dto_is_legal_as_a_request_body() {
    let list = blueprint(
        vec![note(), ack()],
        vec![route(
            TypeRef::Array {
                element: Box::new(TypeRef::Named("Note".to_string())),
                len: None,
            },
            TypeRef::Named("Ack".to_string()),
        )],
    );
    check(&list, &borrowed_config()).expect("each element borrows from the same body");
}

#[test]
fn an_array_of_a_borrowed_dto_is_rejected_as_a_response() {
    let list = blueprint(
        vec![note(), ack()],
        vec![route(
            TypeRef::Named("Ack".to_string()),
            TypeRef::Array {
                element: Box::new(TypeRef::Named("Note".to_string())),
                len: None,
            },
        )],
    );
    let error = check(&list, &borrowed_config()).expect_err("the array names the DTO");
    assert_eq!(error.error_code, "E2016");
}

#[test]
fn a_borrowed_dto_as_a_field_type_is_rejected() {
    let mut wrapper = StructDefinition {
        name: "Wrapper".to_string(),
        fields: vec![FieldDefinition {
            name: "note".to_string(),
            type_ref: TypeRef::Named("Note".to_string()),
            is_optional: false,
            is_borrowed: false,
        }],
    };
    wrapper.fields[0].type_ref = TypeRef::Named("Note".to_string());
    let nested = blueprint(
        vec![note(), ack(), wrapper],
        vec![route(
            TypeRef::Named("Ack".to_string()),
            TypeRef::Named("Ack".to_string()),
        )],
    );
    let error = check(&nested, &borrowed_config()).expect_err("a nested borrow has no lifetime");
    assert_eq!(error.error_code, "E2016");
    assert!(
        error.message.contains("Wrapper.note"),
        "the message names the field: {}",
        error.message
    );
}
