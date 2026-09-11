//! Path parameters in the generated crate: the service signature, the axum
//! handler's `Path` extractor, the channel call, and the gRPC payload.

use super::*;

#[test]
fn a_path_parameter_renders_the_service_channel_and_router() {
    let project = generate_project(
        &path_param_blueprint(),
        &RivetConfig::default(),
        Path::new("."),
    )
    .expect("generate");

    // The service function leads with the path parameter.
    assert!(
        project
            .main_rs
            .contains("pub async fn get_order(id: i64) -> serde_json::Value {"),
        "main_rs:\n{}",
        project.main_rs
    );

    // The handler extracts the path parameter before the state.
    assert!(
        project.main_rs.contains(
            "pub async fn get_order<C: channel::Channel>(super::Path(id): super::Path<i64>, super::State(channel): super::State<C>)"
        ),
        "main_rs:\n{}",
        project.main_rs
    );

    // The in-process channel passes the path variable, never an empty list.
    assert!(
        project.main_rs.contains("service::get_order(id).await"),
        "main_rs:\n{}",
        project.main_rs
    );

    // The crate root imports `Path` because a route needs it.
    assert!(
        project.main_rs.contains("extract::{Json, Path, State}"),
        "main_rs:\n{}",
        project.main_rs
    );
}

#[test]
fn a_path_parameter_round_trips_over_grpc() {
    let project = generate_project(&path_param_blueprint(), &grpc_config(), Path::new("."))
        .expect("generate");

    // One argument serializes as itself, not a tuple.
    assert!(
        project
            .main_rs
            .contains("let payload = serde_json::to_string(&id)"),
        "main_rs:\n{}",
        project.main_rs
    );
    // The dispatch decodes the same shape and calls with the path variable.
    assert!(
        project
            .main_rs
            .contains("let id: i64 = serde_json::from_str(payload)"),
        "main_rs:\n{}",
        project.main_rs
    );
    assert!(
        project.main_rs.contains("service::get_order(id).await"),
        "main_rs:\n{}",
        project.main_rs
    );
}

#[test]
fn a_path_parameter_with_a_body_orders_path_params_then_body() {
    // The service signature and the channel call lead with the path parameter
    // in both topologies.
    for (config, label) in [
        (RivetConfig::default(), "in_process"),
        (grpc_config(), "grpc"),
    ] {
        let project = generate_project(&path_param_body_blueprint(), &config, Path::new("."))
            .expect("generate");
        assert!(
            project.main_rs.contains(
                "pub async fn update_order(id: i64, request: String) -> serde_json::Value {"
            ),
            "{label} main_rs:\n{}",
            project.main_rs
        );
        assert!(
            project
                .main_rs
                .contains("service::update_order(id, request).await"),
            "{label} main_rs:\n{}",
            project.main_rs
        );
    }

    // Two arguments serialize as a tuple over gRPC, and the dispatch decodes
    // the same tuple.
    let project = generate_project(&path_param_body_blueprint(), &grpc_config(), Path::new("."))
        .expect("generate");
    assert!(
        project
            .main_rs
            .contains("let payload = serde_json::to_string(&(id, request))"),
        "main_rs:\n{}",
        project.main_rs
    );
    assert!(
        project
            .main_rs
            .contains("let (id, request): (i64, String) = serde_json::from_str(payload)"),
        "main_rs:\n{}",
        project.main_rs
    );
}

#[test]
fn two_path_parameters_extract_as_a_tuple_in_path_order() {
    let project = generate_project(
        &two_path_params_blueprint(),
        &RivetConfig::default(),
        Path::new("."),
    )
    .expect("generate");

    assert!(
        project.main_rs.contains(
            "pub async fn get_user_order(user_id: String, id: i64) -> serde_json::Value {"
        ),
        "main_rs:\n{}",
        project.main_rs
    );
    assert!(
        project.main_rs.contains(
            "super::Path((user_id, id)): super::Path<(String, i64)>, super::State(channel): super::State<C>"
        ),
        "main_rs:\n{}",
        project.main_rs
    );
    assert!(
        project
            .main_rs
            .contains("service::get_user_order(user_id, id).await"),
        "main_rs:\n{}",
        project.main_rs
    );
}
