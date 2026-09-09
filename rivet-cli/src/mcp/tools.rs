//! The tools `rivet mcp` exposes to AI agents (phase 3, pillar 09).
//!
//! Every tool is a thin wrapper over the phase-1 and phase-2 machinery:
//! parsing, the Gauntlet, the MQI audit, and the `.rivet/` vector index.
//! A tool never re-implements pipeline logic; it calls the same functions
//! the CLI commands call and returns the result as JSON text (or compact
//! markdown for a session) so any MCP client can act on it. Errors
//! serialize as the same structured diagnostics the CLI prints, one JSON
//! object per diagnostic.
use crate::commands::audit::audit_json;
use crate::commands::explain::{explain_async, route_summaries};
use crate::commands::session::render_context;
use crate::diagnostic::Diagnostic;
use crate::parser::python::parse_python_file;
use crate::store;
use crate::store::vector::{index_blueprint, search};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{tool, tool_router};
use serde::Deserialize;
use serde_json::Value;
use std::path::Path;

/// A tool call that names the DSL module that owns the project store.
#[derive(Debug, Clone, Deserialize)]
pub struct AppParams {
    /// Path to the DSL module, for example `app.py`. The store and vector
    /// index live next to it in `.rivet/`.
    pub app: String,
}

/// A tool call that names the module and a symptom to search for.
#[derive(Debug, Clone, Deserialize)]
pub struct SymptomParams {
    /// Path to the DSL module, for example `app.py`.
    pub app: String,
    /// The symptom to search for, for example "orders endpoint 500".
    pub symptom: String,
}

// The JsonSchema impls are written by hand: the schemars derive expands to
// generated code that calls `unwrap`, which clippy bans, and lint allows on
// the struct do not reach macro-expanded code. A `Schema` wraps a JSON
// value, so a flat object of string fields is a few lines of JSON.
impl rmcp::schemars::JsonSchema for AppParams {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "AppParams".into()
    }

    fn json_schema(_: &mut rmcp::schemars::SchemaGenerator) -> rmcp::schemars::Schema {
        string_object_schema(&["app"])
    }
}

impl rmcp::schemars::JsonSchema for SymptomParams {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "SymptomParams".into()
    }

    fn json_schema(_: &mut rmcp::schemars::SchemaGenerator) -> rmcp::schemars::Schema {
        string_object_schema(&["app", "symptom"])
    }
}

/// The JSON schema of an object whose listed fields are all strings.
fn string_object_schema(fields: &[&str]) -> rmcp::schemars::Schema {
    let mut properties = serde_json::Map::new();
    for field in fields {
        let mut string_type = serde_json::Map::new();
        string_type.insert("type".into(), Value::String("string".into()));
        properties.insert((*field).to_string(), Value::Object(string_type));
    }
    let required: Vec<Value> = fields
        .iter()
        .map(|f| Value::String((*f).to_string()))
        .collect();
    let mut object = serde_json::Map::new();
    object.insert("type".into(), Value::String("object".into()));
    object.insert("properties".into(), Value::Object(properties));
    object.insert("required".into(), Value::Array(required));
    object.into()
}

/// The Rivet MCP server: one router, one method per tool.
#[derive(Debug, Clone)]
pub struct RivetTools;

#[tool_router(server_handler)]
impl RivetTools {
    /// Parse a Rivet DSL module and return its blueprint and Gauntlet
    /// findings.
    #[tool(
        description = "Parse a Rivet DSL module and return its IR blueprint and Gauntlet findings as JSON"
    )]
    fn parse_app(&self, Parameters(params): Parameters<AppParams>) -> String {
        result_text(parse_payload(&params.app))
    }

    /// Audit a Rivet DSL module and return the MQI grade and breakdown.
    #[tool(
        description = "Run the phase-1 audit on a Rivet DSL module and return the MQI grade as JSON"
    )]
    fn audit_app(&self, Parameters(params): Parameters<AppParams>) -> String {
        match audit_json(Path::new(&params.app)) {
            Ok(payload) => payload,
            Err(diagnostics) => diagnostics_json(diagnostics).to_string(),
        }
    }

    /// Return the compact markdown context of a Rivet DSL module.
    #[tool(
        description = "Render the module summary, gauntlet config, and findings of a Rivet DSL module as compact markdown"
    )]
    fn session_context(&self, Parameters(params): Parameters<AppParams>) -> String {
        match render_context(Path::new(&params.app)) {
            Ok(markdown) => markdown,
            Err(diagnostic) => diagnostic.to_json(),
        }
    }

    /// List the recorded CLI invocations for a project.
    #[tool(
        description = "List the most recent CLI invocations recorded in the project store as JSON"
    )]
    fn history(&self, Parameters(params): Parameters<AppParams>) -> String {
        result_text(history_payload(&params.app))
    }

    /// Search the blueprint vector index for routes near a symptom.
    #[tool(
        description = "Embed a symptom and return the nearest indexed route chunks with distances as JSON"
    )]
    async fn vector_search(&self, Parameters(params): Parameters<SymptomParams>) -> String {
        result_text(vector_search_payload(&params.app, &params.symptom).await)
    }

    /// Explain a symptom: nearest route, introducing commit, and digest.
    #[tool(
        description = "Trace a symptom to its nearest route, introducing commit, and module digest as JSON"
    )]
    async fn explain_symptom(&self, Parameters(params): Parameters<SymptomParams>) -> String {
        match explain_async(&params.symptom, Path::new(&params.app)).await {
            Ok(explanation) => explanation_json(&params.symptom, explanation).to_string(),
            Err(diagnostics) => diagnostics_json(diagnostics).to_string(),
        }
    }
}

/// The project directory that owns the app's store.
fn project_dir(app_file: &Path) -> std::path::PathBuf {
    app_file
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| ".".into())
}

/// The app label under which the vector index stores its table.
fn app_label(app_file: &Path) -> String {
    app_file
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("app")
        .to_string()
}

/// Parse the module, run the Gauntlet, and fold the results into one JSON
/// payload: the file, the serialized blueprint, and the findings.
fn parse_payload(app_file: &str) -> Result<Value, Vec<Diagnostic>> {
    let path = Path::new(app_file);
    if !path.exists() {
        return Err(vec![Diagnostic::blocker(
            "E1008",
            format!("{app_file} not found"),
        )]);
    }
    let config = crate::config::RivetConfig::load(&project_dir(path))
        .map_err(|message| vec![Diagnostic::blocker("E1008", message)])?;
    let module = parse_python_file(path).map_err(|diagnostic| vec![diagnostic])?;
    let findings = crate::gauntlet::run_gauntlet(&module, &config.gauntlet);

    let blueprint = serde_json::to_value(&module.blueprint).map_err(|err| {
        vec![Diagnostic::blocker(
            "E1008",
            format!("failed to serialize the blueprint: {err}"),
        )]
    })?;
    let findings: Vec<Value> = findings.iter().map(Diagnostic::to_json_value).collect();

    let mut object = serde_json::Map::new();
    object.insert("file".into(), Value::String(module.file));
    object.insert("blueprint".into(), blueprint);
    object.insert("findings".into(), Value::Array(findings));
    Ok(Value::Object(object))
}

/// The recorded command log of a project as a JSON array.
fn history_payload(app_file: &str) -> Result<Value, Vec<Diagnostic>> {
    let conn = store::open(&project_dir(Path::new(app_file)))?;
    let records = store::list_commands(&conn, 20)?;
    let rows: Vec<Value> = records
        .iter()
        .map(|record| {
            let mut object = serde_json::Map::new();
            object.insert("id".into(), Value::from(record.id));
            object.insert("command".into(), Value::String(record.command.clone()));
            object.insert("invoked_at".into(), Value::from(record.invoked_at));
            object.insert(
                "exit_status".into(),
                record.exit_status.map(Value::from).unwrap_or(Value::Null),
            );
            object.insert(
                "duration_ms".into(),
                record.duration_ms.map(Value::from).unwrap_or(Value::Null),
            );
            Value::Object(object)
        })
        .collect();
    Ok(Value::Array(rows))
}

/// Re-index the module and search its blueprint chunks for the symptom.
async fn vector_search_payload(app_file: &str, symptom: &str) -> Result<Value, Vec<Diagnostic>> {
    let path = Path::new(app_file);
    if !path.exists() {
        return Err(vec![Diagnostic::blocker(
            "E1008",
            format!("{app_file} not found"),
        )]);
    }
    let module = parse_python_file(path).map_err(|diagnostic| vec![diagnostic])?;
    let routes = route_summaries(&module);
    let dir = project_dir(path);

    index_blueprint(&dir, &app_label(path), &routes)
        .await
        .map_err(|err| vec![Diagnostic::blocker("E3005", err)])?;
    let hits = search(&dir, &app_label(path), symptom)
        .await
        .map_err(|err| vec![Diagnostic::blocker("E3005", err)])?;

    let hits: Vec<Value> = hits
        .iter()
        .map(|(route, distance)| {
            let mut object = serde_json::Map::new();
            object.insert("route".into(), Value::String(route.clone()));
            object.insert("distance".into(), Value::from(f64::from(*distance)));
            Value::Object(object)
        })
        .collect();
    let mut object = serde_json::Map::new();
    object.insert("symptom".into(), Value::String(symptom.to_string()));
    object.insert("hits".into(), Value::Array(hits));
    Ok(Value::Object(object))
}

/// Render an explanation as a JSON payload.
fn explanation_json(symptom: &str, explanation: crate::commands::explain::Explanation) -> Value {
    let mut object = serde_json::Map::new();
    object.insert("symptom".into(), Value::String(symptom.to_string()));
    object.insert(
        "route".into(),
        explanation.route.map(Value::String).unwrap_or(Value::Null),
    );
    object.insert(
        "distance".into(),
        explanation
            .distance
            .map(|d| Value::from(f64::from(d)))
            .unwrap_or(Value::Null),
    );
    object.insert(
        "introducer".into(),
        explanation
            .introducer
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    object.insert(
        "commit".into(),
        explanation.commit.map(Value::String).unwrap_or(Value::Null),
    );
    object.insert("digest".into(), Value::String(explanation.digest));
    object.insert("findings".into(), Value::from(explanation.findings));
    Value::Object(object)
}

/// The JSON text of a payload, or the structured diagnostics on failure.
fn result_text(result: Result<Value, Vec<Diagnostic>>) -> String {
    match result {
        Ok(payload) => payload.to_string(),
        Err(diagnostics) => diagnostics_json(diagnostics).to_string(),
    }
}

/// Diagnostics as a JSON array, built from the structured payloads.
fn diagnostics_json(diagnostics: Vec<Diagnostic>) -> Value {
    let values: Vec<Value> = diagnostics.iter().map(Diagnostic::to_json_value).collect();
    Value::Array(values)
}

#[cfg(test)]
mod tests {
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
}
