use super::*;
use crate::config;
use crate::test_support::ScratchDir;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

/// The fixture app both transports build and answer.
const FIXTURE_APP: &str = "from rivet import api\n\n@api.get(\"/ping\", stories=[\"US-001\"])\ndef ping() -> dict:\n    return {\"status\": \"pong\"}\n\n@api.post(\"/echo\", stories=[\"US-002\"])\ndef echo(request: dict) -> dict:\n    return {\"echo\": request}\n";

#[test]
fn gauntlet_blocker_stops_build_without_writing_a_crate() {
    let dir = ScratchDir::new("build-storyless");
    let app = dir.join("app.py");
    fs::write(
        &app,
        "from rivet import api\n\n@api.get(\"/ping\")\ndef ping() -> dict:\n    return {\"status\": \"pong\"}\n",
    )
    .expect("write app.py");
    let diagnostics = run_build(&app, BuildTarget::Native).expect_err("storyless route must fail");
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].error_code, "E2045");
    assert!(!diagnostics[0].suggested_fix.is_empty());
    assert!(!dir.join("generated").exists(), "no crate may be written");
}

#[test]
fn dead_code_warns_through_the_gate_with_default_config() {
    // A helper nothing calls is a warning, not a blocker, so the build gate
    // must not fail on it. Driving run_build past the gate would compile the
    // generated crate, so assert the gate decision instead.
    let dir = ScratchDir::new("build-deadcode");
    let app = dir.join("app.py");
    fs::write(
        &app,
        "from rivet import api\n\ndef stale(value: int) -> int:\n    return value\n\n@api.get(\"/ping\", stories=[\"US-1\"])\ndef ping() -> dict:\n    return {\"status\": \"pong\"}\n",
    )
    .expect("write app.py");
    let module = parse_python_file(&app).expect("module parses");
    let findings = gauntlet::run_gauntlet(&module, &config::RivetConfig::default().gauntlet);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].severity, Severity::Warning);
    assert_eq!(findings[0].error_code, "E2044");
}

/// Build the fixture for `mode`, run it, and return the two route responses.
///
/// The gRPC run binds the channel port too, so the probe also proves the
/// channel the HTTP handlers call through is live.
fn build_run_and_probe(
    dir: &ScratchDir,
    mode: &str,
    http_port: u16,
    grpc_port: u16,
) -> (String, String) {
    let app = dir.join("app.py");
    fs::write(&app, FIXTURE_APP).expect("write app.py");
    fs::write(
        dir.join("rivet.toml"),
        format!(
            "[project]\nname = \"transport\"\n\n[transport]\nmode = \"{mode}\"\ngrpc_port = {grpc_port}\n\n[environments]\ndevelopment = {{ host = \"127.0.0.1\", port = {http_port} }}\n"
        ),
    )
    .expect("write rivet.toml");

    // `in_process` first, then `grpc`: the second build reuses the first
    // crate's dependency cache, so the transport flag is the only change.
    run_build(&app, BuildTarget::Native).expect("the fixture must build");

    let binary = dir.join("generated/target/release/transport");
    let _server = Server(
        Command::new(&binary)
            .env("HOST", "127.0.0.1")
            .env("PORT", http_port.to_string())
            .spawn()
            .expect("start the generated server"),
    );
    (
        http_get(http_port, "/ping", None),
        http_get(http_port, "/echo", Some("{\"hello\":\"world\"}")),
    )
}

/// The running generated server; the guard stops it on drop, on success and
/// while a panic unwinds, so no test leaves a server behind.
struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Ask the running server for one route and return its response body.
fn http_get(port: u16, path: &str, body: Option<&str>) -> String {
    let request = match body {
        Some(body) => format!(
            "POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {len}\r\nConnection: close\r\n\r\n{body}",
            len = body.len(),
        ),
        None => get_request(path, ""),
    };
    http_exchange(port, &request)
        .split_once("\r\n\r\n")
        .map(|(_, body)| body.to_string())
        .unwrap_or_default()
}

/// A `GET` request head with an extra header line, or an empty extra.
fn get_request(path: &str, extra: &str) -> String {
    format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\n{extra}Connection: close\r\n\r\n")
}

/// Send one request and return the raw response bytes.
///
/// The compressed probe needs the bytes: a Brotli body is not UTF-8.
fn http_exchange_bytes(port: u16, request: &str) -> Vec<u8> {
    wait_for_port(port);
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect to the server");
    stream
        .write_all(request.as_bytes())
        .expect("send the request");
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .expect("read the response");
    response
}

/// Send one request and return the whole response, headers included.
fn http_exchange(port: u16, request: &str) -> String {
    String::from_utf8_lossy(&http_exchange_bytes(port, request)).into_owned()
}

/// The body length of a raw response, measured from its own header end.
fn body_len(response: &[u8]) -> usize {
    response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map_or(0, |end| response.len() - (end + 4))
}

/// The value of a response header, matched without case, or an empty string.
fn header<'a>(response: &'a str, name: &str) -> &'a str {
    response
        .lines()
        .take_while(|line| !line.is_empty())
        .filter_map(|line| line.split_once(':'))
        .find(|(found, _)| found.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.trim())
        .unwrap_or_default()
}

/// Wait until the server accepts connections.
fn wait_for_port(port: u16) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("the generated server never listened on port {port}");
}

#[test]
fn one_blueprint_answers_over_both_transports() {
    let dir = ScratchDir::new("transport-both");

    let (in_process_ping, in_process_echo) = build_run_and_probe(&dir, "in_process", 4321, 54321);
    let (grpc_ping, grpc_echo) = build_run_and_probe(&dir, "grpc", 4322, 54322);

    assert!(
        in_process_ping.contains("{\"status\":\"pong\"}"),
        "in-process /ping: {in_process_ping}"
    );
    assert!(
        in_process_echo.contains("{\"echo\":{\"hello\":\"world\"}}"),
        "in-process /echo: {in_process_echo}"
    );
    // The gRPC run reaches the same service functions through the channel it
    // serves, so both topologies answer identically.
    assert!(
        grpc_ping.contains("{\"status\":\"pong\"}"),
        "grpc /ping: {grpc_ping}"
    );
    assert!(
        grpc_echo.contains("{\"echo\":{\"hello\":\"world\"}}"),
        "grpc /echo: {grpc_echo}"
    );
    assert_eq!(in_process_ping, grpc_ping);
    assert_eq!(in_process_echo, grpc_echo);
}

/// The fixture frontend build the asset test embeds: a realistic page, big
/// enough and repetitive enough that Brotli shrinks it on the wire.
const FIXTURE_INDEX: &str = concat!(
    "<!doctype html>\n",
    "<html lang=\"en\">\n",
    "  <head>\n",
    "    <meta charset=\"utf-8\" />\n",
    "    <title>fixture frontend</title>\n",
    "  </head>\n",
    "  <body>\n",
    "    <h1>Embedded page</h1>\n",
    "    <p>The generated binary serves this page from its own memory.</p>\n",
    "    <p>The generated binary serves this page from its own memory.</p>\n",
    "    <p>The generated binary serves this page from its own memory.</p>\n",
    "    <p>The generated binary serves this page from its own memory.</p>\n",
    "    <p>The generated binary serves this page from its own memory.</p>\n",
    "    <p>The generated binary serves this page from its own memory.</p>\n",
    "  </body>\n",
    "</html>\n",
);

/// A fixture asset, so the test proves the content type too.
const FIXTURE_ASSET: &str = "console.log(\"embedded asset\");\n";

#[test]
fn the_binary_serves_the_embedded_frontend_after_dist_is_renamed() {
    let dir = ScratchDir::new("embedded-assets");
    let app = dir.join("app.py");
    fs::write(&app, FIXTURE_APP).expect("write app.py");
    fs::write(
        dir.join("rivet.toml"),
        "[project]\nname = \"embedded\"\n\n[frontend]\ndist = \"dist\"\n\n[environments]\ndevelopment = { host = \"127.0.0.1\", port = 4331 }\n",
    )
    .expect("write rivet.toml");
    fs::create_dir_all(dir.join("dist/assets")).expect("create dist");
    fs::write(dir.join("dist/index.html"), FIXTURE_INDEX).expect("write index.html");
    fs::write(dir.join("dist/assets/main.js"), FIXTURE_ASSET).expect("write asset");

    run_build(&app, BuildTarget::Native).expect("the fixture must build");

    // The directory is gone, so only a binary that carries the build inside
    // can still answer.
    fs::rename(dir.join("dist"), dir.join("dist-renamed")).expect("rename dist");

    let _server = Server(
        Command::new(dir.join("generated/target/release/embedded"))
            .env("HOST", "127.0.0.1")
            .env("PORT", "4331")
            .spawn()
            .expect("start the generated server"),
    );

    let root = http_exchange(4331, &get_request("/", ""));
    assert!(root.starts_with("HTTP/1.1 200"), "{root}");
    assert!(root.ends_with(FIXTURE_INDEX), "{root}");
    assert!(
        header(&root, "content-type").contains("text/html"),
        "{root}"
    );

    let asset = http_exchange(4331, &get_request("/assets/main.js", ""));
    assert!(asset.starts_with("HTTP/1.1 200"), "{asset}");
    assert!(
        header(&asset, "content-type").contains("javascript"),
        "{asset}"
    );
    assert!(asset.ends_with(FIXTURE_ASSET), "{asset}");

    // A blueprint route wins over an asset path of the same shape.
    let route = http_exchange(4331, &get_request("/ping", ""));
    assert!(route.ends_with("{\"status\":\"pong\"}"), "{route}");

    // A browser navigation to a client-side route falls back to the embedded
    // index; anything else keeps the 404 it asks for.
    let navigation = "Accept: text/html,application/xhtml+xml\r\n";
    let deep = http_exchange(4331, &get_request("/orders/42", navigation));
    assert!(deep.starts_with("HTTP/1.1 200"), "{deep}");
    assert!(deep.ends_with(FIXTURE_INDEX), "{deep}");

    let missing_asset = http_exchange(4331, &get_request("/assets/gone.js", navigation));
    assert!(
        missing_asset.starts_with("HTTP/1.1 404"),
        "a path that names a file must not answer HTML: {missing_asset}"
    );

    let api_typo = http_exchange(4331, &get_request("/api/orders", "Accept: */*\r\n"));
    assert!(
        api_typo.starts_with("HTTP/1.1 404"),
        "a client that does not ask for HTML keeps its 404: {api_typo}"
    );

    // Nothing outside the embedded folder is reachable.
    let traversal = http_exchange(4331, &get_request("/../app.py", navigation));
    assert!(
        !traversal.starts_with("HTTP/1.1 200"),
        "the embedded folder is sealed: {traversal}"
    );

    // A `HEAD` reports the length of the file it would send.
    let head = http_exchange(
        4331,
        "HEAD /assets/main.js HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
    );
    assert!(head.starts_with("HTTP/1.1 200"), "{head}");
    assert_eq!(
        header(&head, "content-length"),
        FIXTURE_ASSET.len().to_string().as_str(),
        "{head}"
    );

    // Revalidation with the returned ETag answers `304` and sends no body.
    let etag = header(&root, "etag").to_string();
    let fresh = http_exchange(
        4331,
        &get_request("/", &format!("If-None-Match: {etag}\r\n")),
    );
    assert!(fresh.starts_with("HTTP/1.1 304"), "{fresh}");
    assert!(!fresh.contains("embedded page"), "{fresh}");

    // A client that accepts Brotli receives the compressed page. The probe
    // uses the index: `tower-http` never compresses a body under 32 bytes,
    // so the 31-byte asset stays as it is.
    let compressed = http_exchange_bytes(4331, &get_request("/", "Accept-Encoding: br\r\n"));
    let head = String::from_utf8_lossy(&compressed).into_owned();
    assert!(head.starts_with("HTTP/1.1 200"), "{head}");
    assert_eq!(header(&head, "content-encoding"), "br", "{head}");
    assert!(
        body_len(&compressed) < FIXTURE_INDEX.len(),
        "compression shrinks the body: {head}"
    );
}

#[test]
fn the_admin_panel_serves_the_route_table_and_the_panel() {
    let dir = ScratchDir::new("admin-panel");
    let app = dir.join("app.py");
    fs::write(&app, FIXTURE_APP).expect("write app.py");
    fs::write(
        dir.join("rivet.toml"),
        "[project]\nname = \"panel\"\n\n[admin]\nenabled = true\n\n[environments]\ndevelopment = { host = \"127.0.0.1\", port = 4332 }\n",
    )
    .expect("write rivet.toml");

    run_build(&app, BuildTarget::Native).expect("the fixture must build");

    let _server = Server(
        Command::new(dir.join("generated/target/release/panel"))
            .env("HOST", "127.0.0.1")
            .env("PORT", "4332")
            .spawn()
            .expect("start the generated server"),
    );

    let routes = http_exchange(4332, &get_request("/__rivet/routes", ""));
    assert!(routes.starts_with("HTTP/1.1 200"), "{routes}");
    assert!(
        header(&routes, "content-type").contains("application/json"),
        "{routes}"
    );
    assert!(routes.contains("\"method\":\"GET\""), "{routes}");
    assert!(routes.contains("\"path\":\"/ping\""), "{routes}");
    assert!(routes.contains("\"handler\":\"ping\""), "{routes}");
    assert!(routes.contains("\"stories\":[\"US-001\"]"), "{routes}");
    assert!(routes.contains("\"path\":\"/echo\""), "{routes}");

    let panel = http_exchange(4332, &get_request("/__rivet/", "Accept: text/html\r\n"));
    assert!(panel.starts_with("HTTP/1.1 200"), "{panel}");
    assert!(
        header(&panel, "content-type").contains("text/html"),
        "{panel}"
    );
    assert!(panel.contains("Rivet routes"), "{panel}");

    // The panel adds endpoints; it does not take the app's own routes away.
    let ping = http_exchange(4332, &get_request("/ping", ""));
    assert!(ping.ends_with("{\"status\":\"pong\"}"), "{ping}");
}

mod borrow;
mod collisions;
mod registry;
mod wasm;
