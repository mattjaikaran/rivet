use super::*;

/// A fixture app module with one story-tagged route.
fn write_fixture(dir: &std::path::Path) -> std::path::PathBuf {
    let app = dir.join("app.py");
    std::fs::write(
        &app,
        "from rivet import api\n\n@api.get(\"/ping\", stories=[\"US-1\"])\ndef ping() -> dict:\n    return {\"status\": \"pong\"}\n",
    )
    .expect("write fixture");
    app
}

#[test]
fn parse_app_returns_blueprint_and_findings() {
    let dir = crate::mcp::tests::scratch("mcp-parse");
    let app = write_fixture(&dir);

    let payload = parse_payload(app.to_str().expect("utf8 path")).expect("parse ok");
    let object = payload.as_object().expect("payload is an object");
    assert!(object.get("blueprint").is_some(), "blueprint present");
    let findings = object
        .get("findings")
        .and_then(Value::as_array)
        .expect("findings array");
    assert!(findings.is_empty(), "clean fixture has no findings");
}

#[test]
fn parse_app_reports_a_missing_file_as_diagnostics() {
    let text = result_text(parse_payload("/nonexistent/app.py"));
    let value: Value = serde_json::from_str(&text).expect("diagnostics parse");
    let array = value.as_array().expect("diagnostics array");
    assert_eq!(array.len(), 1);
    assert_eq!(
        array[0].get("error_code").and_then(Value::as_str),
        Some("E1008")
    );
    let fix = array[0]
        .get("suggested_fix")
        .and_then(Value::as_str)
        .expect("diagnostic carries a suggested_fix");
    assert!(!fix.is_empty(), "missing-file fix is concrete");
}

#[test]
fn audit_app_returns_the_grade() {
    let dir = crate::mcp::tests::scratch("mcp-audit");
    let app = write_fixture(&dir);

    let text = audit_json(&app).expect("audit runs");
    let payload: Value = serde_json::from_str(&text).expect("audit parses");
    assert_eq!(
        payload.get("grade").and_then(Value::as_str),
        Some("A+"),
        "clean fixture grades A+"
    );
}

#[test]
fn session_and_history_read_the_project_store() {
    let dir = crate::mcp::tests::scratch("mcp-store");
    let app = write_fixture(&dir);

    // Seed the store with one finished invocation, the way main() does.
    let project_dir = project_dir(&app);
    let conn = store::open(&project_dir).expect("open store");
    let id = store::start_command(&conn, "build").expect("start command");
    store::finish_command(&conn, id, 0, std::time::Duration::from_millis(3))
        .expect("finish command");

    let session = render_context(&app).expect("session renders");
    assert!(
        session.contains("# Rivet session for"),
        "markdown context renders"
    );
    assert!(session.contains("app.py"), "context names the app");

    let history = history_payload(app.to_str().expect("utf8 path")).expect("history ok");
    let rows = history.as_array().expect("history array");
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].get("command").and_then(Value::as_str),
        Some("build")
    );
    assert_eq!(rows[0].get("exit_status"), Some(&Value::from(0)));
}

#[tokio::test]
async fn vector_search_ranks_the_fixture_route() {
    let dir = crate::mcp::tests::scratch("mcp-vector");
    let app = write_fixture(&dir);

    let payload = vector_search_payload(app.to_str().expect("utf8 path"), "ping status pong")
        .await
        .expect("search ok");
    let hits = payload
        .get("hits")
        .and_then(Value::as_array)
        .expect("hits array");
    assert!(!hits.is_empty(), "the fixture route matches its symptom");
    let best = hits.first().expect("best hit");
    let route = best.get("route").and_then(Value::as_str).expect("route");
    assert!(route.contains("/ping"), "nearest route is /ping: {route}");
}

#[tokio::test]
async fn explain_symptom_reports_route_and_digest() {
    let dir = crate::mcp::tests::scratch("mcp-explain");
    let app = write_fixture(&dir);

    let explanation = explain_async("ping status", &app)
        .await
        .expect("explain ok");
    let payload = explanation_json("ping status", explanation);
    let route = payload.get("route").and_then(Value::as_str).expect("route");
    assert!(route.contains("/ping"), "route names /ping: {route}");
    assert_eq!(
        payload.get("findings").and_then(Value::as_u64),
        Some(0),
        "clean fixture has no findings"
    );
    assert!(payload.get("digest").is_some(), "digest present");
}
