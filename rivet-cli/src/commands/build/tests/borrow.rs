//! The zero-copy route, end to end on the native target.
//!
//! A unit test on generated text cannot prove a borrow: the claim is that the
//! generated crate compiles with a `&'a str` field and that the route answers
//! a real request. This test builds a small app whose DTO borrows, runs the
//! binary, and posts a body — the same probe the WASM protocol test runs on
//! the module.

use super::*;

/// A fixture app with one route whose request DTO borrows from the body.
const FIXTURE_APP: &str = "from rivet import api\n\nclass Note:\n    text: borrowed[str]\n\n@api.post(\"/notes\", stories=[\"US-004\"])\ndef create_note(request: Note) -> dict:\n    return {\"echo\": request}\n";

#[test]
fn a_borrowed_request_body_answers_over_http() {
    let dir = ScratchDir::new("zero-copy-native");
    let app = dir.join("app.py");
    fs::write(&app, FIXTURE_APP).expect("write app.py");
    fs::write(
        dir.join("rivet.toml"),
        "[project]\nname = \"zerocopy\"\n\n[rust_native_features]\nzero_copy_deserialization = true\n",
    )
    .expect("write rivet.toml");
    run_build(&app, BuildTarget::Native, true).expect("the borrowed fixture must build");

    let port = 31390;
    let binary = dir.join("generated/target/release/zerocopy");
    let _server = Server(
        Command::new(&binary)
            .env("HOST", "127.0.0.1")
            .env("PORT", port.to_string())
            .spawn()
            .expect("start the generated server"),
    );

    let answer = http_get(port, "/notes", Some("{\"text\":\"borrowed\"}"));
    assert!(
        answer.contains("\"borrowed\""),
        "the borrowed text round-trips: {answer}"
    );

    // A body that does not match the DTO answers 400, not a panic: the
    // handler decodes the bytes itself and maps the decode error.
    let rejected = http_exchange(
        port,
        "POST /notes HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: 4\r\nConnection: close\r\n\r\nnope",
    );
    assert!(
        rejected.starts_with("HTTP/1.1 400"),
        "an unreadable body answers 400: {rejected}"
    );
}
