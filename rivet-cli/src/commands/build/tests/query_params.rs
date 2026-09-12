//! Query parameters, end to end on the native target.
//!
//! A unit test on generated text cannot prove a query parameter: the claim is
//! that a parameter the path does not name is read from the query string as
//! its declared type, that a missing or unparseable value is a `400` (the
//! route matched, the query did not), and that an extra key is ignored. This
//! test builds a small app with one query-carrying route, runs the binary, and
//! probes real HTTP for every status the router and handler must agree on.

use super::*;

/// A fixture app with one route that carries two integer and one string query
/// parameter.
const FIXTURE_APP: &str = "from rivet import api\n\n@api.get(\"/search\", stories=[\"US-020\"])\ndef search(page: int, size: int, name: str) -> dict:\n    return {\"page\": page, \"size\": size, \"name\": name}\n";

#[test]
fn query_parameters_answer_over_http() {
    let dir = ScratchDir::new("query-params-native");
    let app = dir.join("app.py");
    fs::write(&app, FIXTURE_APP).expect("write app.py");
    fs::write(dir.join("rivet.toml"), "[project]\nname = \"search\"\n").expect("write rivet.toml");
    run_build(&app, BuildTarget::Native, true).expect("the query-param fixture must build");

    let port = 31392;
    let binary = dir.join("generated/target/release/search");
    let _server = Server(
        Command::new(&binary)
            .env("HOST", "127.0.0.1")
            .env("PORT", port.to_string())
            .spawn()
            .expect("start the generated server"),
    );

    // Every query value reaches the service as its declared type.
    let answer = http_get(port, "/search?page=2&size=10&name=ada", None);
    let parsed: serde_json::Value =
        serde_json::from_str(&answer).unwrap_or_else(|err| panic!("{err}: {answer}"));
    assert_eq!(
        parsed.get("page").and_then(serde_json::Value::as_i64),
        Some(2),
        "the page round-trips: {answer}"
    );
    assert_eq!(
        parsed.get("size").and_then(serde_json::Value::as_i64),
        Some(10),
        "the size round-trips: {answer}"
    );
    assert_eq!(
        parsed.get("name").and_then(serde_json::Value::as_str),
        Some("ada"),
        "the name round-trips: {answer}"
    );

    // A query with no parameters answers 400, not 404: the route matched,
    // the required parameter is missing.
    let missing = http_exchange(port, &get_request("/search", ""));
    assert!(
        missing.starts_with("HTTP/1.1 400"),
        "a missing query parameter answers 400, not 404: {missing}"
    );

    // A value that cannot parse as its declared type answers 400.
    let unparseable = http_exchange(port, &get_request("/search?page=abc&size=10&name=ada", ""));
    assert!(
        unparseable.starts_with("HTTP/1.1 400"),
        "an unparseable query value answers 400: {unparseable}"
    );

    // An unknown extra key is ignored.
    let extra = http_get(port, "/search?page=2&size=10&name=ada&extra=1", None);
    let parsed_extra: serde_json::Value =
        serde_json::from_str(&extra).unwrap_or_else(|err| panic!("{err}: {extra}"));
    assert_eq!(
        parsed_extra.get("name").and_then(serde_json::Value::as_str),
        Some("ada"),
        "an unknown query key is ignored: {extra}"
    );

    // The path matches but the method does not: 405.
    let wrong_method = http_exchange(
        port,
        "POST /search?page=2&size=10&name=ada HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    );
    assert!(
        wrong_method.starts_with("HTTP/1.1 405"),
        "a wrong method answers 405: {wrong_method}"
    );

    // A repeated key is last-wins: the extraction collects the query into a
    // map, so the later `page=3` overwrites `page=2`. The request therefore
    // answers 200 with the last value, not 400.
    let repeated = http_get(port, "/search?page=2&page=3&size=10&name=ada", None);
    let parsed_repeated: serde_json::Value =
        serde_json::from_str(&repeated).unwrap_or_else(|err| panic!("{err}: {repeated}"));
    assert_eq!(
        parsed_repeated
            .get("page")
            .and_then(serde_json::Value::as_i64),
        Some(3),
        "a repeated key keeps the last value: {repeated}"
    );
}
