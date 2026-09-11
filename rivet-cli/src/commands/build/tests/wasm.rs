//! The WebAssembly target: the generated module answers the same routes.
//!
//! The module is an edge handler, so the test drives the request protocol:
//! one JSON request in, one JSON envelope out. The generated crate is a
//! binary either way, so the test builds and runs it for the host first —
//! that proves the dispatch, the status codes, and the envelope without a
//! WASI host — and then runs the compiled module under Wasmtime when the
//! host is installed.

use super::*;

/// A fixture app with a parameterless route, a body-taking route, and a DTO
/// that carries a fixed-size array.
///
/// The array field proves the serde bridge travels into the wasm crate too:
/// the DTO renderer is shared, so a bridge emitted for one target and not the
/// other would fail to compile here.
const FIXTURE_APP: &str = "from typing import List\n\nfrom rivet import api\n\n@api.get(\"/ping\", stories=[\"US-001\"])\ndef ping() -> dict:\n    return {\"status\": \"pong\"}\n\n@api.post(\"/echo\", stories=[\"US-002\"])\ndef echo(request: dict) -> dict:\n    return {\"echo\": request}\n\nclass Embedding:\n    values: List[float, 4]\n\n@api.post(\"/embed\", stories=[\"US-003\"])\ndef embed(request: Embedding) -> Embedding:\n    return request\n";

/// Build the fixture for the wasm target and answer the module path.
fn build_wasm(dir: &ScratchDir, name: &str) -> std::path::PathBuf {
    let app = dir.join("app.py");
    fs::write(&app, FIXTURE_APP).expect("write app.py");
    fs::write(
        dir.join("rivet.toml"),
        format!("[project]\nname = \"{name}\"\n\n[rust_native_features]\nconst_generics = true\n"),
    )
    .expect("write rivet.toml");
    run_build(&app, BuildTarget::Wasm).expect("the wasm fixture must build");
    dir.join("generated-wasm")
        .join("target")
        .join("wasm32-wasip1")
        .join("release")
        .join(format!("{name}.wasm"))
}

/// Run one request through a command, and answer its stdout.
fn run_with_stdin(mut command: std::process::Command, request: &str) -> String {
    use std::process::Stdio;

    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().expect("start the module");
    child
        .stdin
        .as_mut()
        .expect("stdin is piped")
        .write_all(request.as_bytes())
        .expect("write the request");
    let output = child.wait_with_output().expect("read the answer");
    assert!(
        output.status.success(),
        "the module exits 0: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// The `status` of one envelope.
fn status(envelope: &str) -> u64 {
    let parsed: serde_json::Value =
        serde_json::from_str(envelope).unwrap_or_else(|err| panic!("{err}: {envelope}"));
    parsed
        .get("status")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or_else(|| panic!("the envelope carries a status: {envelope}"))
}

/// The `body` of one envelope.
fn body(envelope: &str) -> serde_json::Value {
    let parsed: serde_json::Value =
        serde_json::from_str(envelope).unwrap_or_else(|err| panic!("{err}: {envelope}"));
    parsed
        .get("body")
        .cloned()
        .unwrap_or_else(|| panic!("the envelope carries a body: {envelope}"))
}

/// The five protocol answers, asserted against one runner.
fn assert_protocol(run: impl Fn(&str) -> String) {
    let ping = run("{\"method\":\"GET\",\"path\":\"/ping\"}");
    assert_eq!(status(&ping), 200, "{ping}");
    let pong = body(&ping);
    assert_eq!(
        pong.get("status").and_then(serde_json::Value::as_str),
        Some("pong"),
        "a parameterless route answers its value: {ping}"
    );

    let echo =
        run("{\"method\":\"POST\",\"path\":\"/echo\",\"body\":\"{\\\"hello\\\":\\\"world\\\"}\"}");
    assert_eq!(status(&echo), 200, "{echo}");
    assert_eq!(
        body(&echo)
            .get("echo")
            .and_then(|echo| echo.get("hello"))
            .and_then(serde_json::Value::as_str),
        Some("world"),
        "the request body round-trips: {echo}"
    );

    let unreadable = run("not json");
    assert_eq!(status(&unreadable), 400, "{unreadable}");

    // A fixed-size array field round-trips through the shared serde bridge,
    // on both the host build and the module.
    let array = run(
        "{\"method\":\"POST\",\"path\":\"/embed\",\"body\":\"{\\\"values\\\":[0.5,0.25,0.125,1.0]}\"}",
    );
    assert_eq!(status(&array), 200, "{array}");
    assert_eq!(
        body(&array)
            .get("values")
            .and_then(serde_json::Value::as_array)
            .map(Vec::len),
        Some(4),
        "the array keeps its declared length: {array}"
    );

    let short =
        run("{\"method\":\"POST\",\"path\":\"/embed\",\"body\":\"{\\\"values\\\":[0.5]}\"}");
    assert_eq!(status(&short), 400, "a short array is rejected: {short}");

    // A host that sends the body as a JSON value means the same request.
    let parsed = run("{\"method\":\"POST\",\"path\":\"/echo\",\"body\":{\"hello\":\"world\"}}");
    assert_eq!(status(&parsed), 200, "{parsed}");
    assert_eq!(
        body(&parsed)
            .get("echo")
            .and_then(|echo| echo.get("hello"))
            .and_then(serde_json::Value::as_str),
        Some("world"),
        "a parsed body round-trips: {parsed}"
    );

    let missing = run("{\"method\":\"GET\",\"path\":\"/nope\"}");
    assert_eq!(status(&missing), 404, "{missing}");

    let wrong_method = run("{\"method\":\"DELETE\",\"path\":\"/ping\"}");
    assert_eq!(status(&wrong_method), 405, "{wrong_method}");
    assert!(
        body(&wrong_method)
            .get("error")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|message| message.contains("GET")),
        "the 405 answer names the allowed methods: {wrong_method}"
    );

    let unreadable = run("not json");
    assert_eq!(status(&unreadable), 400, "{unreadable}");
}

#[test]
fn the_generated_module_answers_the_request_protocol() {
    let dir = ScratchDir::new("wasm-protocol");
    build_wasm(&dir, "wasmhost");

    // Run the same crate for the host: the dispatch, the statuses, and the
    // envelope are the module's own logic, and this needs no WASI host.
    let binary = dir
        .join("generated-wasm")
        .join("target")
        .join("release")
        .join("wasmhost");
    cargo(&dir.join("generated-wasm"), &["build", "--release"]);
    assert!(
        binary.is_file(),
        "the host build wrote {}",
        binary.display()
    );

    assert_protocol(|request| {
        let command = std::process::Command::new(&binary);
        run_with_stdin(command, request)
    });
}

#[test]
fn the_module_runs_under_wasmtime() {
    if !wasmtime_available() {
        // The module is verified for the host above; this half needs the
        // WASI host, and the repository records its absence rather than
        // skipping the module's only real platform check silently.
        eprintln!(
            "skipping the Wasmtime check: install `wasmtime` (for example `brew install wasmtime`) to run the module on its real target"
        );
        return;
    }
    let dir = ScratchDir::new("wasm-wasmtime");
    let module = build_wasm(&dir, "wasmtime-app");
    assert!(
        module.is_file(),
        "the wasm build wrote {}",
        module.display()
    );

    assert_protocol(|request| {
        let command = std::process::Command::new("wasmtime");
        let mut command = command;
        command.arg("run").arg(&module);
        run_with_stdin(command, request)
    });
}

/// Whether `wasmtime` is on the PATH.
fn wasmtime_available() -> bool {
    std::process::Command::new("wasmtime")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// Run one cargo command in `dir`, and assert that it succeeds.
fn cargo(dir: &Path, args: &[&str]) {
    let status = std::process::Command::new("cargo")
        .args(args)
        .current_dir(dir)
        .status()
        .expect("run cargo");
    assert!(status.success(), "cargo {args:?} failed");
}
