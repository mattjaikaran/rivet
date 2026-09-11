//! `rivet trace "<symptom>"`: follow a symptom from the DSL module through
//! the code the transpiler would generate, and back to the commit that
//! introduced it (phase 3).
//!
//! The trace reuses the `explain` pipeline. The symptom is embedded with the
//! same deterministic character-trigram hash the vector index uses, so the
//! nearest route chunk is the one whose path, handler, and story IDs share
//! the most tokens with the symptom — that is why a symptom like "orders"
//! picks the `/orders` route over unrelated routes. The matched chunk names
//! a handler, and the trace walks that handler through three hops:
//!
//! 1. the DSL declaration that defines it (the parse tree), for the source
//!    line;
//! 2. the axum registration and the service function that
//!    [`generate_project`] would render — generated in memory and scanned,
//!    never compiled, so the trace needs no cargo run;
//! 3. the git commit that first added the handler's name to the module,
//!    via the same `git log -S` pickaxe `explain` uses.
//!
//! Every hop is read-only: the trace writes no crate and no source file.

use super::explain::explain_data;
use crate::config::RivetConfig;
use crate::diagnostic::Diagnostic;
use crate::parser::python::{DeclKind, parse_python_file};
use crate::store;
use crate::transpiler::rust::generate_project;
use std::path::Path;

/// Run `trace` and print the report for a human.
pub fn run_trace(symptom: &str, app_file: &Path) -> Result<(), Vec<Diagnostic>> {
    print!("{}", build_trace(symptom, app_file)?);
    Ok(())
}

/// Build the trace report, one fact per line.
fn build_trace(symptom: &str, app_file: &Path) -> Result<String, Vec<Diagnostic>> {
    let project_dir = store::project_dir_for(app_file);

    // Parse the module and load the `[gauntlet]` config exactly like
    // `rivet build`, so the DSL line numbers and the generated code come
    // from the same source of truth the compiler would see.
    let config = RivetConfig::load(&project_dir).map_err(|message| {
        vec![Diagnostic::blocker(
            "E1008",
            message,
            "correct the invalid `rivet.toml` value the message names (or fix the file's permissions), then rerun the command",
        )]
    })?;

    if !app_file.exists() {
        return Err(vec![
            Diagnostic::blocker(
                "E1008",
                format!("{} not found", app_file.display()),
                "write a `from rivet import api` module, then run `rivet trace`",
            )
            .located(project_dir.display().to_string(), 1),
        ]);
    }

    let module = parse_python_file(app_file).map_err(|diagnostic| vec![diagnostic])?;
    let explanation = explain_data(symptom, app_file)?;

    // No route matched the symptom at all: report and stop.
    let Some(chunk) = explanation.route.as_deref() else {
        return Ok(format!("Trace: no route matches {symptom:?}\n"));
    };

    // The chunk is "METHOD path handler NAME stories ...".
    let handler = chunk
        .split_whitespace()
        .skip_while(|word| *word != "handler")
        .nth(1)
        .ok_or_else(|| {
            stale_index(format!(
                "the matched route chunk does not name a handler: {chunk}"
            ))
        })?;

    let route = module
        .blueprint
        .routes
        .iter()
        .find(|route| route.handler_name == handler)
        .ok_or_else(|| {
            stale_index(format!(
                "the matched chunk names handler `{handler}`, which has no route in {}",
                module.file
            ))
        })?;
    let declaration = module
        .decls
        .iter()
        .find(|decl| decl.kind == DeclKind::Route && decl.name == handler)
        .ok_or_else(|| {
            stale_index(format!(
                "handler `{handler}` has a route but no DSL declaration in {}",
                module.file
            ))
        })?;

    // Render what the transpiler would generate, without compiling.
    let generated = generate_project(&module.blueprint, &config, &project_dir)
        .map_err(|diagnostic| vec![diagnostic])?;
    let service_needle = format!("fn {handler}(");
    let registration = rendered_line(
        &generated.main_rs,
        "axum registration for the route",
        |line| line.trim_start().starts_with(".route(") && line.contains(route.path.as_str()),
    )?;
    // The route's logic now lives in the transport-free service layer, which
    // the axum handler and the channel both call.
    let service_fn = rendered_line(&generated.main_rs, "service function", |line| {
        line.contains(service_needle.as_str())
    })?;

    let source_line = module
        .source
        .split('\n')
        .nth(declaration.line - 1)
        .unwrap_or("");

    let mut out = String::new();
    out.push_str(&format!("Trace: {symptom:?}\n"));
    out.push_str(&format!(
        "DSL: {} {} -> {}() at {}:{}\n",
        route.method.as_str(),
        route.path,
        route.handler_name,
        app_file.display(),
        declaration.line
    ));
    out.push_str(&format!("  {source_line}\n"));
    out.push_str(&format!("Generated: {registration}\n"));
    out.push_str(&format!("Service: {service_fn}\n"));
    match &explanation.introducer {
        Some(introducer) => out.push_str(&format!("Introduced by commit: {introducer}\n")),
        None => out.push_str(
            "Could not find the commit that introduced this handler (not a git checkout?)\n",
        ),
    }
    match &explanation.commit {
        Some(commit) => {
            out.push_str(&format!(
                "Current commit {commit}, module digest {}\n",
                explanation.digest
            ));
        }
        None => out.push_str("Not a git checkout; no commit fingerprint recorded\n"),
    }
    if explanation.findings == 0 {
        out.push_str("Gauntlet: no findings on the module\n");
    } else {
        out.push_str(&format!(
            "Gauntlet: {} finding(s) on the module\n",
            explanation.findings
        ));
    }
    Ok(out)
}

/// A matched chunk that the parsed module cannot reconcile is a stale
/// vector-store row; surface it as a blocker so the trace never guesses.
fn stale_index(what: impl Into<String>) -> Diagnostic {
    Diagnostic::blocker(
        "E3005",
        what,
        "rerun `rivet trace`; the vector index rebuilds from the module on every run, and report the issue if it persists",
    )
}

/// Find one expected generated line.
///
/// [`generate_project`] always renders one `.route(...)` registration and
/// one handler function per blueprint route, so a miss means the renderer
/// is out of step with the IR, not that the route is absent.
fn rendered_line<'a>(
    main_rs: &'a str,
    expected: &str,
    matches: impl Fn(&str) -> bool,
) -> Result<&'a str, Diagnostic> {
    main_rs.lines().find(|line| matches(line)).ok_or_else(|| {
        Diagnostic::blocker(
            "E2009",
            format!("generated main.rs does not contain the expected {expected}"),
            "report this as a generator bug: `generate_project` did not render the traced route, even though the module parsed",
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::ScratchDir;
    use std::fs;
    use std::path::Path;
    use std::process::Command;

    /// Commit 1: the ping route only, mirroring examples/basic.
    const PING_ONLY: &str = concat!(
        "from rivet import api\n",
        "\n",
        "@api.get(\"/ping\", stories=[\"US-001\"])\n",
        "def ping() -> dict:\n",
        "    return {\"status\": \"pong\"}\n",
    );

    /// Commit 2 adds the orders route; this is the handler the trace hunts.
    const ORDERS_ROUTE: &str = concat!(
        "@api.get(\"/orders\", stories=[\"US-2\"])\n",
        "def orders() -> dict:\n",
        "    return {\"orders\": []}\n",
    );

    /// A fresh git fixture under the system temp dir, with a local identity.
    fn fixture() -> ScratchDir {
        let dir = ScratchDir::new("trace-introducing-commit");
        git(&dir, &["init"]);
        git(&dir, &["config", "user.name", "Rivet Trace Test"]);
        git(&dir, &["config", "user.email", "rivet-trace@example.com"]);
        dir
    }

    /// Run git in the fixture dir, asserting success and returning stdout.
    fn git(dir: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .expect("run git");
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    /// Commit the current app.py. Signing is disabled so the test never
    /// depends on a host gpg setup.
    fn commit(dir: &Path, message: &str) {
        git(dir, &["add", "app.py"]);
        git(
            dir,
            &["-c", "commit.gpgsign=false", "commit", "-m", message],
        );
    }

    #[test]
    fn trace_walks_orders_symptom_to_its_introducing_commit() {
        let dir = fixture();
        let app = dir.join("app.py");

        fs::write(&app, PING_ONLY).expect("write the commit 1 app.py");
        commit(&dir, "add the ping route");

        let with_orders = format!("{PING_ONLY}\n{ORDERS_ROUTE}");
        fs::write(&app, with_orders).expect("write the commit 2 app.py");
        commit(&dir, "add the orders route");
        // The introducing commit of the orders handler is commit 2.
        let commit2 = git(&dir, &["rev-parse", "HEAD"]);

        let text = build_trace("orders", &app).expect("trace succeeds");
        assert!(text.contains("/orders"), "trace text:\n{text}");
        assert!(
            text.contains("DSL: GET /orders -> orders()"),
            "trace text:\n{text}"
        );
        assert!(
            text.contains("@api.get(\"/orders\", stories=[\"US-2\"])"),
            "trace text:\n{text}"
        );
        assert!(
            text.contains(&format!("Introduced by commit: {commit2}")),
            "trace text:\n{text}"
        );
        // The generated service function, as the generator renders a
        // `-> dict` route: async fn orders() -> serde_json::Value. The trace
        // scans for it before the handler module, so this also proves the
        // block order `{service}` then `{handlers}` still holds.
        assert!(
            text.contains("async fn orders() -> serde_json::Value {"),
            "trace text:\n{text}"
        );
        // The router reaches the handler through the module (pillar 02), so
        // the registration names `handlers::` — the change that keeps every
        // handler name out of the crate root.
        assert!(
            text.contains(".route(\"/orders\", get(handlers::orders::<channel::InProcess>))"),
            "trace text:\n{text}"
        );
        assert!(text.contains("Current commit "), "trace text:\n{text}");
        assert!(text.contains("Gauntlet:"), "trace text:\n{text}");
    }
}
