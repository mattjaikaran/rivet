//! The `[rust_native_features]` tests: the flags and what each one renders.

use super::fixtures::{RustNativeFeatures, ping_blueprint};
use super::*;
use rivet_core::ir::{Expr, HttpMethod, RequestSpec, ResponseSpec, RouteDefinition, Stmt, TypeRef};

/// A blueprint whose DTO carries a fixed-size array.
fn embedding_blueprint() -> ServiceBlueprint {
    ServiceBlueprint {
        name: "app".to_string(),
        structs: vec![StructDefinition {
            name: "Embedding".to_string(),
            fields: vec![FieldDefinition {
                name: "values".to_string(),
                type_ref: TypeRef::Array {
                    element: Box::new(TypeRef::Float),
                    len: Some(768),
                },
                is_optional: false,
                is_borrowed: false,
            }],
        }],
        routes: vec![RouteDefinition {
            method: HttpMethod::Post,
            path: "/embed".to_string(),
            path_params: vec![],
            query_params: vec![],
            handler_name: "embed".to_string(),
            stories: vec!["US-003".to_string()],
            middlewares: vec![],
            request: RequestSpec::Json {
                var: "request".to_string(),
                ty: TypeRef::Named("Embedding".to_string()),
            },
            response: ResponseSpec::Json(TypeRef::Named("Embedding".to_string())),
            body: vec![Stmt::Return(Expr::Ident("request".to_string()))],
        }],
        dependencies: vec![],
    }
}

/// A config that opts into const generics.
fn const_generics_config() -> RivetConfig {
    RivetConfig {
        rust_native_features: RustNativeFeatures {
            const_generics: true,
            ..RustNativeFeatures::default()
        },
        ..RivetConfig::default()
    }
}

#[test]
fn const_generics_renders_a_fixed_size_array_with_its_serde_bridge() {
    let project = generate_project(
        &embedding_blueprint(),
        &const_generics_config(),
        Path::new("."),
    )
    .expect("generate");

    // The field keeps the array type, and borrows the bridge because serde
    // derives no impl past 32 elements.
    assert!(
        project.main_rs.contains("pub values: [f64; 768],"),
        "main_rs:\n{}",
        project.main_rs
    );
    assert!(
        project
            .main_rs
            .contains("#[serde(with = \"rivet_fixed_array\")]\n    pub values: [f64; 768],"),
        "main_rs:\n{}",
        project.main_rs
    );
    assert!(project.main_rs.contains("mod rivet_fixed_array {"));
    assert!(
        project.main_rs.contains("values.try_into()"),
        "the bridge rejects a length that does not match"
    );
    assert!(
        !project.main_rs.contains("E2003"),
        "the opted-in path generates instead of failing"
    );
}

#[test]
fn a_fixed_size_array_without_the_opt_in_still_reports_e2003() {
    let error = match generate_project(
        &embedding_blueprint(),
        &RivetConfig::default(),
        Path::new("."),
    ) {
        Ok(_) => panic!("the flag gates the feature"),
        Err(error) => error,
    };
    assert!(
        error.suggested_fix.contains("const_generics"),
        "the fix names the flag that turns it on: {}",
        error.suggested_fix
    );
    assert!(
        !error.message.contains("not generated yet"),
        "the message names the opt-in, not a missing feature: {}",
        error.message
    );
}

#[test]
fn a_plain_array_needs_no_bridge() {
    let project = generate_project(&ping_blueprint(), &const_generics_config(), Path::new("."))
        .expect("generate");
    assert!(
        !project.main_rs.contains("mod rivet_fixed_array"),
        "the bridge is emitted only when a DTO declares one"
    );
}

#[test]
fn a_flag_the_generator_does_not_implement_blocks_the_build() {
    let config = RivetConfig {
        rust_native_features: RustNativeFeatures {
            compile_time_rbac: true,
            ..RustNativeFeatures::default()
        },
        ..RivetConfig::default()
    };
    let error = match generate_project(&ping_blueprint(), &config, Path::new(".")) {
        Ok(_) => panic!("an unimplemented flag must not pass silently"),
        Err(error) => error,
    };
    assert_eq!(error.error_code, "E2013");
    assert!(
        error.message.contains("compile_time_rbac"),
        "{}",
        error.message
    );
    assert!(
        error.suggested_fix.contains("compile_time_rbac = false"),
        "the fix names the flag: {}",
        error.suggested_fix
    );
}

#[test]
fn an_optional_fixed_size_array_is_rejected_before_cargo_sees_it() {
    let mut blueprint = embedding_blueprint();
    blueprint.structs[0].fields[0].is_optional = true;
    let error = match generate_project(&blueprint, &const_generics_config(), Path::new(".")) {
        Ok(_) => panic!("the combination is not generated"),
        Err(error) => error,
    };
    assert_eq!(error.error_code, "E2012");
    assert!(
        error.suggested_fix.contains("Optional"),
        "the fix names the wrapper to drop: {}",
        error.suggested_fix
    );
}

#[test]
fn an_array_literal_must_match_a_fixed_size_declaration() {
    let mut blueprint = embedding_blueprint();
    blueprint.routes[0].body = vec![Stmt::Return(Expr::Construct {
        ty: "Embedding".to_string(),
        args: vec![(
            "values".to_string(),
            Expr::Array(vec![Expr::Float(0.5), Expr::Float(0.5)]),
        )],
    })];
    let error = match generate_project(&blueprint, &const_generics_config(), Path::new(".")) {
        Ok(_) => panic!("a two-value literal does not fill a 768-element array"),
        Err(error) => error,
    };
    assert_eq!(error.error_code, "E2011");
    assert!(error.message.contains("768"), "{}", error.message);
}

/// A blueprint with two routes on one method and path, with differing bodies
/// so the Verifier's duplicate rule does not catch them.
fn duplicate_route_blueprint() -> ServiceBlueprint {
    let mut blueprint = ping_blueprint();
    blueprint.routes.push(RouteDefinition {
        method: HttpMethod::Get,
        path: "/ping".to_string(),
        path_params: vec![],
        query_params: vec![],
        handler_name: "ping_again".to_string(),
        stories: vec!["US-002".to_string()],
        middlewares: vec![],
        request: RequestSpec::None,
        response: ResponseSpec::Json(TypeRef::Json),
        body: vec![Stmt::Return(Expr::Object(vec![(
            "status".to_string(),
            Expr::Str("alive".to_string()),
        )]))],
    });
    blueprint
}

#[test]
fn two_routes_on_one_method_and_path_are_rejected() {
    let error = match generate_project(
        &duplicate_route_blueprint(),
        &RivetConfig::default(),
        Path::new("."),
    ) {
        Ok(_) => panic!("the native router would panic at startup"),
        Err(error) => error,
    };
    assert_eq!(error.error_code, "E2014");
    assert!(error.message.contains("GET /ping"), "{}", error.message);
    assert!(
        error.message.contains("`ping`") && error.message.contains("`ping_again`"),
        "the message names both handlers: {}",
        error.message
    );
    assert!(
        error.file.is_none(),
        "the generator holds no app path to name"
    );
    assert!(!error.suggested_fix.is_empty());
}

#[test]
fn the_wasm_target_rejects_the_same_duplicate() {
    // Both targets must agree: the native router panics on the overlap, and
    // the wasm dispatch would silently keep the first arm.
    let error = match generate_wasm_project(&duplicate_route_blueprint(), &RivetConfig::default()) {
        Ok(_) => panic!("the dispatch would carry two identical arms"),
        Err(error) => error,
    };
    assert_eq!(error.error_code, "E2014");
    assert!(error.message.contains("GET /ping"), "{}", error.message);
}

#[test]
fn the_same_path_on_two_methods_is_not_a_duplicate() {
    let mut blueprint = ping_blueprint();
    blueprint.routes.push(RouteDefinition {
        method: HttpMethod::Post,
        path: "/ping".to_string(),
        path_params: vec![],
        query_params: vec![],
        handler_name: "create_ping".to_string(),
        stories: vec!["US-002".to_string()],
        middlewares: vec![],
        request: RequestSpec::None,
        response: ResponseSpec::Json(TypeRef::Json),
        body: vec![Stmt::Return(Expr::Object(vec![(
            "status".to_string(),
            Expr::Str("created".to_string()),
        )]))],
    });
    let project = generate_project(&blueprint, &RivetConfig::default(), Path::new("."))
        .expect("a GET and a POST on one path are two routes");
    assert!(
        project
            .main_rs
            .contains("post(handlers::create_ping::<channel::InProcess>)")
    );
}
