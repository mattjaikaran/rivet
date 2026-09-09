//! The tools `rivet mcp` exposes to AI agents (phase 3, pillar 09).
//!
//! Every tool is a thin wrapper over the phase-1 and phase-2 machinery:
//! parsing, the Gauntlet, the MQI audit, and the `.rivet/` vector index.
//! A tool never re-implements pipeline logic; it calls the same functions
//! the CLI commands call and returns the result as JSON text so any MCP
//! client can act on it. Errors serialize as the same structured
//! diagnostics the CLI prints, one JSON object per diagnostic.

use crate::config::RivetConfig;
use crate::diagnostic::Diagnostic;
use crate::gauntlet;
use crate::parser::python::parse_python_file;
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
    pub fn parse_app(&self, Parameters(params): Parameters<AppParams>) -> String {
        result_text(parse_payload(&params.app))
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
    let config = RivetConfig::load(&project_dir(path))
        .map_err(|message| vec![Diagnostic::blocker("E1008", message)])?;
    let module = parse_python_file(path).map_err(|diagnostic| vec![diagnostic])?;
    let findings = gauntlet::run_gauntlet(&module, &config.gauntlet);

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

/// Render a tool result as text: the JSON payload, or the structured
/// diagnostics when the call failed.
fn result_text(result: Result<Value, Vec<Diagnostic>>) -> String {
    match result {
        Ok(payload) => payload.to_string(),
        Err(diagnostics) => {
            let values: Vec<Value> = diagnostics.iter().map(Diagnostic::to_json_value).collect();
            Value::Array(values).to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_app_returns_blueprint_and_findings() {
        let dir = crate::mcp::tests::scratch("mcp-parse");
        let app = dir.join("app.py");
        std::fs::write(
            &app,
            "from rivet import api\n\n@api.get(\"/ping\", stories=[\"US-1\"])\ndef ping() -> dict:\n    return {\"status\": \"pong\"}\n",
        )
        .expect("write fixture");

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
}
