//! Path parameters, end to end on the native target.
//!
//! A unit test on generated text cannot prove a path parameter: the claim is
//! that the route matches one non-empty segment, that the segment reaches the
//! service as the declared type, and that a value which fails to parse is a
//! `400`, not a `404`. This test builds a small app with one parameterised
//! route, runs the binary, and probes real HTTP for every status the router
//! and handler must agree on.

use super::*;

/// A fixture app with one route that carries an integer path parameter.
const FIXTURE_APP: &str = "from rivet import api\n\n@api.get(\"/orders/{id}\", stories=[\"US-010\"])\ndef get_order(id: int) -> dict:\n    return {\"id\": id}\n";

#[test]
fn a_path_parameter_answers_over_http() {
    let dir = ScratchDir::new("path-params-native");
    let app = dir.join("app.py");
    fs::write(&app, FIXTURE_APP).expect("write app.py");
    fs::write(dir.join("rivet.toml"), "[project]\nname = \"orders\"\n").expect("write rivet.toml");
    run_build(&app, BuildTarget::Native).expect("the path-param fixture must build");

    let port = 31391;
    let binary = dir.join("generated/target/release/orders");
    let _server = Server(
        Command::new(&binary)
            .env("HOST", "127.0.0.1")
            .env("PORT", port.to_string())
            .spawn()
            .expect("start the generated server"),
    );

    // One non-empty segment matches, and the value reaches the service as the
    // declared integer.
    let answer = http_get(port, "/orders/42", None);
    assert!(
        answer.contains("{\"id\":42}"),
        "the id round-trips: {answer}"
    );

    // A value that cannot parse answers 400: the route matched, the segment
    // did not.
    let unparseable = http_exchange(port, &get_request("/orders/abc", ""));
    assert!(
        unparseable.starts_with("HTTP/1.1 400"),
        "an unparseable segment answers 400, not 404: {unparseable}"
    );

    // A path with a different segment count misses the route.
    let fewer_segments = http_exchange(port, &get_request("/orders", ""));
    assert!(
        fewer_segments.starts_with("HTTP/1.1 404"),
        "a missing segment misses the route: {fewer_segments}"
    );

    // A trailing slash adds an empty segment and misses too.
    let trailing = http_exchange(port, &get_request("/orders/42/", ""));
    assert!(
        trailing.starts_with("HTTP/1.1 404"),
        "a trailing slash misses the route: {trailing}"
    );

    // An empty segment in the middle misses as well.
    let empty_segment = http_exchange(port, &get_request("/orders//42", ""));
    assert!(
        empty_segment.starts_with("HTTP/1.1 404"),
        "an empty segment misses the route: {empty_segment}"
    );

    // The path matches but the method does not: 405.
    let wrong_method = http_exchange(
        port,
        "POST /orders/42 HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    );
    assert!(
        wrong_method.starts_with("HTTP/1.1 405"),
        "a wrong method answers 405: {wrong_method}"
    );
}
