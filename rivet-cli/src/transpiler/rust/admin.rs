//! The built-in admin panel: the route table and the single-file panel.
//!
//! `[admin] enabled = true` in `rivet.toml` mounts two read-only endpoints on
//! the generated app: `GET /__rivet/routes` answers the route table the
//! blueprint declares, and `GET /__rivet/` answers a dependency-free HTML
//! panel that renders it.
//!
//! The table is rendered from the blueprint at build time, so the endpoint
//! serves one static string: no serialization, no state, and no per-request
//! map. The panel is one embedded HTML file. Rivet has no Node toolchain in
//! its build, so there is no React or Solid build step (pillar 03).
//!
//! The rendered module is a template with `@@NAME@@` tokens instead of a
//! `format!` string: the panel is mostly braces, and the tokens keep it
//! readable as Rust.

use crate::diagnostic::Diagnostic;
use rivet_core::ir::ServiceBlueprint;
use rivet_core::reserved;
use serde_json::Value;

/// The generated admin wiring for `main.rs`.
///
/// Every field is empty when the project does not enable the panel.
#[derive(Debug)]
pub(super) struct Wiring {
    /// The `mod admin` block.
    pub(super) module: String,
    /// The router lines that mount the panel.
    pub(super) routes: String,
}

/// Render the generated admin wiring, or empty wiring when `enabled` is
/// false.
///
/// Returns an [`E2006`](Diagnostic::blocker) diagnostic when the blueprint
/// declares one of the panel paths, because axum panics on an overlapping
/// route and the build must reject that instead.
pub(super) fn render(blueprint: &ServiceBlueprint, enabled: bool) -> Result<Wiring, Diagnostic> {
    if !enabled {
        return Ok(Wiring {
            module: String::new(),
            routes: String::new(),
        });
    }
    if let Some(collision) = PANEL_PATHS
        .iter()
        .find(|panel| blueprint.routes.iter().any(|route| route.path == **panel))
    {
        return Err(Diagnostic::blocker(
            "E2006",
            format!("the blueprint declares `{collision}`, which the admin panel already serves"),
            "rename the route so it does not begin with `/__rivet`, or set `[admin] enabled = false` in `rivet.toml`, then rerun the command",
        )
        .located("rivet.toml", 1));
    }
    Ok(Wiring {
        module: MODULE
            .replace("@@MODULE@@", reserved::ADMIN_MODULE)
            .replace("@@ROUTES@@", &super::rust_str(&route_table(blueprint))),
        routes: ROUTES.replace("@@MODULE@@", reserved::ADMIN_MODULE),
    })
}

/// The paths the panel owns. The `__rivet` prefix keeps them out of a
/// project's own route space.
const PANEL_PATHS: &[&str] = &["/__rivet/routes", "/__rivet/"];

/// The router lines that mount the panel.
const ROUTES: &str = "\n        .route(\"/__rivet/routes\", get(@@MODULE@@::routes))\n        .route(\"/__rivet/\", get(@@MODULE@@::panel))";

/// The blueprint's route table as a JSON array: method, path, handler, and
/// the story IDs the route answers to.
///
/// Built from [`serde_json::Value`] constructors only, which are infallible,
/// so this function cannot panic (the project bans `unwrap` and `expect`).
fn route_table(blueprint: &ServiceBlueprint) -> String {
    let rows: Vec<Value> = blueprint
        .routes
        .iter()
        .map(|route| {
            let mut row = serde_json::Map::new();
            row.insert("method".into(), Value::String(route.method.as_str().into()));
            row.insert("path".into(), Value::String(route.path.clone()));
            row.insert("handler".into(), Value::String(route.handler_name.clone()));
            row.insert(
                "stories".into(),
                Value::Array(
                    route
                        .stories
                        .iter()
                        .map(|story| Value::String(story.clone()))
                        .collect(),
                ),
            );
            Value::Object(row)
        })
        .collect();
    Value::Array(rows).to_string()
}

/// The admin module every panel-enabled app carries.
const MODULE: &str = r##"/// The built-in admin panel (phase 4, pillar 03).
///
/// The route table is rendered from the blueprint at build time, so the
/// endpoint answers one static string the compiler put in the binary. The
/// panel is a single embedded HTML file: no dependencies, no build step.
mod @@MODULE@@ {
    use axum::body::Body;
    use axum::http::{header, HeaderValue};
    use axum::response::Response;

    /// The blueprint's route table: method, path, handler, stories.
    const ROUTES: &str = @@ROUTES@@;

    /// The single-file panel that renders the route table.
    const PANEL: &str = r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Rivet routes</title>
<style>
:root { color-scheme: light dark; }
body { margin: 0; padding: 2rem; font: 14px/1.5 ui-monospace, SFMono-Regular, Menlo, monospace; }
h1 { font-size: 1.1rem; margin: 0 0 1rem; }
p { opacity: .7; }
table { border-collapse: collapse; width: 100%; }
th, td { padding: .4rem .6rem; text-align: left; border-bottom: 1px solid color-mix(in srgb, currentColor 20%, transparent); }
th { font-weight: 600; }
td.method { font-weight: 700; }
</style>
</head>
<body>
<h1>Rivet routes</h1>
<p id="summary">Loading...</p>
<table>
<thead><tr><th>Method</th><th>Path</th><th>Handler</th><th>Stories</th></tr></thead>
<tbody id="rows"></tbody>
</table>
<script>
var rows = document.getElementById("rows");
var summary = document.getElementById("summary");
fetch("/__rivet/routes")
  .then(function (response) { return response.json(); })
  .then(function (routes) {
    summary.textContent = routes.length + " route(s)";
    routes.forEach(function (route) {
      var row = document.createElement("tr");
      [route.method, route.path, route.handler, route.stories.join(", ")].forEach(function (value, index) {
        var cell = document.createElement("td");
        if (index === 0) { cell.className = "method"; }
        cell.textContent = value;
        row.appendChild(cell);
      });
      rows.appendChild(row);
    });
  })
  .catch(function (error) { summary.textContent = String(error); });
</script>
</body>
</html>
"#;

    /// The route table as JSON.
    pub(super) async fn routes() -> Response {
        respond("application/json", ROUTES)
    }

    /// The panel.
    pub(super) async fn panel() -> Response {
        respond("text/html; charset=utf-8", PANEL)
    }

    /// A static response with its content type, never cached.
    fn respond(content_type: &'static str, body: &'static str) -> Response {
        let mut response = Response::new(Body::from(body));
        // The default status is `200`, so it is already correct.
        let headers = response.headers_mut();
        headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        response
    }
}
"##;

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_core::ir::{Expr, HttpMethod, RequestSpec, ResponseSpec, RouteDefinition, TypeRef};

    fn blueprint() -> ServiceBlueprint {
        ServiceBlueprint {
            name: "app".to_string(),
            structs: vec![],
            routes: vec![RouteDefinition {
                method: HttpMethod::Post,
                path: "/echo".to_string(),
                path_params: vec![],
                query_params: vec![],
                handler_name: "echo".to_string(),
                stories: vec!["US-002".to_string(), "US-003".to_string()],
                middlewares: vec![],
                request: RequestSpec::None,
                response: ResponseSpec::Json(TypeRef::Json),
                returns: vec![Expr::Null],
            }],
            dependencies: vec![],
        }
    }

    #[test]
    fn disabled_renders_nothing() {
        let wiring = render(&blueprint(), false).expect("render");
        assert!(wiring.module.is_empty(), "no module without the opt-in");
        assert!(wiring.routes.is_empty(), "no routes without the opt-in");
    }

    #[test]
    fn the_table_names_every_route_field() {
        let blueprint = blueprint();
        let json = route_table(&blueprint);
        assert!(json.contains("\"method\":\"POST\""), "{json}");
        assert!(json.contains("\"path\":\"/echo\""), "{json}");
        assert!(json.contains("\"handler\":\"echo\""), "{json}");
        assert!(
            json.contains("\"stories\":[\"US-002\",\"US-003\"]"),
            "{json}"
        );

        let wiring = render(&blueprint, true).expect("render");
        assert!(
            wiring.module.contains(&super::super::rust_str(&json)),
            "the module carries the table as a string literal"
        );
        assert!(wiring.routes.contains("/__rivet/routes"));
        assert!(wiring.routes.contains("/__rivet/"));
    }

    #[test]
    fn a_route_on_a_panel_path_is_rejected() {
        let mut blueprint = blueprint();
        blueprint.routes[0].path = "/__rivet/routes".to_string();
        let error = render(&blueprint, true).expect_err("the panel path is taken");
        assert_eq!(error.error_code, "E2006");
        assert!(error.message.contains("/__rivet/routes"));
        assert!(!error.suggested_fix.is_empty());

        assert!(
            render(&blueprint, false).is_ok(),
            "a project without the panel keeps its route"
        );
    }
}
