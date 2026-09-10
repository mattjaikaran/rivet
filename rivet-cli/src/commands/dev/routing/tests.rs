use super::*;

/// A topology with a frontend, so the routing decision is observable.
fn topology(routes: &[&str]) -> DevTopology {
    DevTopology {
        backend: "http://backend".to_string(),
        frontend: Some("http://frontend".to_string()),
        routes: routes.iter().map(|route| route.to_string()).collect(),
    }
}

#[test]
fn declared_routes_and_the_api_prefix_belong_to_the_backend() {
    let topology = topology(&["/ping", "/orders/{id}"]);

    // The blueprint's own routes stay with the backend.
    assert_eq!(upstream_for(&topology, "/ping"), "http://backend");
    assert_eq!(upstream_for(&topology, "/orders/42"), "http://backend");
    // The API prefix is the escape hatch for backend paths outside the
    // blueprint.
    assert_eq!(upstream_for(&topology, "/api/users"), "http://backend");
    assert_eq!(upstream_for(&topology, "/api"), "http://backend");
    // Everything else is the frontend's.
    assert_eq!(upstream_for(&topology, "/"), "http://frontend");
    assert_eq!(upstream_for(&topology, "/assets/app.js"), "http://frontend");
    assert_eq!(upstream_for(&topology, "/apiary"), "http://frontend");
    assert_eq!(upstream_for(&topology, "/ping/extra"), "http://frontend");
}

#[test]
fn without_declared_routes_only_the_api_prefix_reaches_the_backend() {
    let topology = topology(&[]);
    assert_eq!(upstream_for(&topology, "/ping"), "http://frontend");
    assert_eq!(upstream_for(&topology, "/api/ping"), "http://backend");
}

#[test]
fn a_project_without_a_frontend_sends_every_path_to_the_backend() {
    let topology = DevTopology {
        backend: "http://backend".to_string(),
        frontend: None,
        routes: vec!["/ping".to_string()],
    };
    assert_eq!(upstream_for(&topology, "/"), "http://backend");
    assert_eq!(upstream_for(&topology, "/anything"), "http://backend");
}

#[test]
fn the_api_prefix_is_rewritten_away_for_the_backend() {
    let topology = topology(&["/ping"]);

    // The backend serves the blueprint's paths, so the proxy strips the
    // prefix it added.
    assert_eq!(upstream_path(&topology, "/api/ping"), "/ping");
    assert_eq!(upstream_path(&topology, "/api/users/7"), "/users/7");
    assert_eq!(upstream_path(&topology, "/api"), "/");
    // A declared route keeps its path verbatim, `/api` included.
    assert_eq!(upstream_path(&topology, "/ping"), "/ping");
}

#[test]
fn a_declared_route_wins_over_the_api_prefix() {
    let topology = topology(&["/api/orders"]);

    assert_eq!(upstream_for(&topology, "/api/orders"), "http://backend");
    assert_eq!(upstream_path(&topology, "/api/orders"), "/api/orders");
}

#[test]
fn the_upstream_url_keeps_the_query_and_rewrites_the_path() {
    let topology = topology(&["/ping"]);
    let uri: Uri = "/api/users?page=2&size=10".parse().expect("a valid URI");

    assert_eq!(
        upstream_url(&topology, "http://backend", &uri),
        "http://backend/users?page=2&size=10"
    );
}

#[test]
fn route_matching_follows_the_axum_shape() {
    assert!(route_matches("/orders/42", "/orders/{id}"));
    assert!(!route_matches("/orders/42/items", "/orders/{id}"));
    assert!(!route_matches("/orders", "/orders/{id}"));
    assert!(!route_matches("/orders/42", "/orders/{id}/items"));
    assert!(route_matches("/ping", "/ping"));
    assert!(!route_matches("/ping", "/pong"));
}
