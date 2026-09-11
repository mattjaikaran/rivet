//! The `zero_copy_deserialization` flag: a DTO field borrowed from the body.
//!
//! The borrow has exactly one buffer to point into, so the generator accepts a
//! borrowed DTO only as a route's request body. These tests cover the render,
//! the `Bytes` extraction the bound forces, the untouched owned route, the
//! missing opt-in, the rejected wasm response, and the composition with
//! `const_generics`.

use super::fixtures::{RustNativeFeatures, ping_blueprint};
use super::*;
use rivet_core::ir::{Expr, HttpMethod, RequestSpec, ResponseSpec, RouteDefinition, TypeRef};

/// A blueprint whose request DTO borrows its text from the body.
fn borrowed_blueprint() -> ServiceBlueprint {
    ServiceBlueprint {
        name: "app".to_string(),
        structs: vec![StructDefinition {
            name: "Note".to_string(),
            fields: vec![FieldDefinition {
                name: "text".to_string(),
                type_ref: TypeRef::String,
                is_optional: false,
                is_borrowed: true,
            }],
        }],
        routes: vec![RouteDefinition {
            method: HttpMethod::Post,
            path: "/notes".to_string(),
            path_params: vec![],
            query_params: vec![],
            handler_name: "create_note".to_string(),
            stories: vec!["US-004".to_string()],
            middlewares: vec![],
            request: RequestSpec::Json {
                var: "request".to_string(),
                ty: TypeRef::Named("Note".to_string()),
            },
            response: ResponseSpec::Json(TypeRef::Json),
            returns: vec![Expr::Object(vec![(
                "status".to_string(),
                Expr::Str("stored".to_string()),
            )])],
        }],
        dependencies: vec![],
    }
}

/// A config that opts into borrowed request bodies.
fn zero_copy_config() -> RivetConfig {
    RivetConfig {
        rust_native_features: RustNativeFeatures {
            zero_copy_deserialization: true,
            ..RustNativeFeatures::default()
        },
        ..RivetConfig::default()
    }
}

#[test]
fn zero_copy_renders_a_borrowed_field_with_a_lifetime() {
    let project = generate_project(&borrowed_blueprint(), &zero_copy_config(), Path::new("."))
        .expect("generate");

    // The DTO takes the lifetime, and the field is a slice of the body.
    assert!(
        project.main_rs.contains("pub struct Note<'a> {"),
        "main_rs:\n{}",
        project.main_rs
    );
    assert!(
        project
            .main_rs
            .contains("#[serde(borrow)]\n    pub text: &'a str,"),
        "main_rs:\n{}",
        project.main_rs
    );
    assert!(
        !project.main_rs.contains("pub text: String"),
        "the borrowed field must not own its text"
    );
}

#[test]
fn zero_copy_extracts_bytes_because_json_needs_owned_data() {
    let project = generate_project(&borrowed_blueprint(), &zero_copy_config(), Path::new("."))
        .expect("generate");
    assert!(
        project.main_rs.contains("bytes: axum::body::Bytes"),
        "the handler takes the raw body: {}",
        project.main_rs
    );
    assert!(
        project.main_rs.contains("serde_json::from_slice(&bytes)"),
        "the handler decodes the bytes itself"
    );
    assert!(
        !project.main_rs.contains("super::Json(request)"),
        "the Json extractor would require DeserializeOwned"
    );
}

#[test]
fn zero_copy_does_not_touch_an_owned_route() {
    let project =
        generate_project(&ping_blueprint(), &zero_copy_config(), Path::new(".")).expect("generate");
    assert!(
        !project.main_rs.contains("axum::body::Bytes"),
        "no route borrows, so no handler takes bytes"
    );
}

#[test]
fn a_borrowed_field_without_the_opt_in_reports_e2015() {
    let error = match generate_project(
        &borrowed_blueprint(),
        &RivetConfig::default(),
        Path::new("."),
    ) {
        Ok(_) => panic!("the flag gates the feature"),
        Err(error) => error,
    };
    assert_eq!(error.error_code, "E2015");
    assert!(
        error
            .suggested_fix
            .contains("zero_copy_deserialization = true"),
        "the fix names the flag: {}",
        error.suggested_fix
    );
}

#[test]
fn a_borrowed_response_type_is_rejected_on_the_wasm_target() {
    let mut blueprint = borrowed_blueprint();
    blueprint.structs.push(StructDefinition {
        name: "Ack".to_string(),
        fields: vec![FieldDefinition {
            name: "status".to_string(),
            type_ref: TypeRef::String,
            is_optional: false,
            is_borrowed: false,
        }],
    });
    blueprint.routes[0].request = RequestSpec::None;
    blueprint.routes[0].response = ResponseSpec::Json(TypeRef::Named("Note".to_string()));
    let error = match generate_wasm_project(&blueprint, &zero_copy_config()) {
        Ok(_) => panic!("a reply cannot borrow"),
        Err(error) => error,
    };
    assert_eq!(error.error_code, "E2016");
}

#[test]
fn zero_copy_composes_with_const_generics_in_one_dto() {
    let mut blueprint = borrowed_blueprint();
    blueprint.structs[0].fields.push(FieldDefinition {
        name: "values".to_string(),
        type_ref: TypeRef::Array {
            element: Box::new(TypeRef::Float),
            len: Some(4),
        },
        is_optional: false,
        is_borrowed: false,
    });
    let config = RivetConfig {
        rust_native_features: RustNativeFeatures {
            zero_copy_deserialization: true,
            const_generics: true,
            ..RustNativeFeatures::default()
        },
        ..RivetConfig::default()
    };

    let project = generate_project(&blueprint, &config, Path::new(".")).expect("generate");
    // The two flags are independent, and one DTO can carry both shapes: the
    // lifetime comes from the borrow, the array from the const generics.
    assert!(
        project.main_rs.contains("pub struct Note<'a> {"),
        "main_rs:\n{}",
        project.main_rs
    );
    assert!(
        project
            .main_rs
            .contains("#[serde(borrow)]\n    pub text: &'a str,"),
        "main_rs:\n{}",
        project.main_rs
    );
    assert!(
        project
            .main_rs
            .contains("#[serde(with = \"rivet_fixed_array\")]\n    pub values: [f64; 4],"),
        "main_rs:\n{}",
        project.main_rs
    );
}
