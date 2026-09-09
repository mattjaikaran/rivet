//! `rivet build`: parse the DSL entry point, run the Gauntlet, generate the
//! Rust crate, and compile it with cargo.
//!
//! The Gauntlet runs between parse and generate. Its blocker findings stop
//! the command before any crate is written; its warnings print to stderr
//! and let the build continue. All diagnostics share the agentic-JSON path
//! so a driver (human or agent) sees every finding at once.

use crate::config::RivetConfig;
use crate::diagnostic::{Diagnostic, Severity};
use crate::gauntlet;
use crate::parser::python::parse_python_file;
use crate::transpiler::rust::generate_project;
use std::path::Path;

pub fn run_build(app_file: &Path) -> Result<(), Vec<Diagnostic>> {
    let project_dir = app_file
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| ".".into());

    let config = RivetConfig::load(&project_dir)
        .map_err(|message| vec![Diagnostic::blocker("E1008", message)])?;

    if !app_file.exists() {
        return Err(vec![
            Diagnostic::blocker("E1008", format!("{} not found", app_file.display())).located(
                project_dir.display().to_string(),
                1,
                Some("write a `from rivet import api` module, then run `rivet build`"),
            ),
        ]);
    }

    let module = parse_python_file(app_file).map_err(|diagnostic| vec![diagnostic])?;

    // The Gauntlet gates generation. Warnings report; blockers stop the
    // build with JSON on stderr and no crate written.
    let mut warnings = Vec::new();
    let mut blockers = Vec::new();
    for diagnostic in gauntlet::run_gauntlet(&module, &config.gauntlet) {
        match diagnostic.severity {
            Severity::Warning => warnings.push(diagnostic),
            Severity::Blocker => blockers.push(diagnostic),
        }
    }
    for warning in &warnings {
        eprintln!("{}", warning.to_json());
        eprintln!("{}", warning.summary());
    }
    if !blockers.is_empty() {
        return Err(blockers);
    }

    let generated =
        generate_project(&module.blueprint, &config).map_err(|diagnostic| vec![diagnostic])?;

    let out_dir = project_dir.join("generated");
    std::fs::create_dir_all(out_dir.join("src")).map_err(|err| {
        vec![Diagnostic::blocker(
            "E1008",
            format!("failed to create the generated directory: {err}"),
        )]
    })?;
    std::fs::write(out_dir.join("Cargo.toml"), &generated.cargo_toml).map_err(|err| {
        vec![Diagnostic::blocker(
            "E1008",
            format!("failed to write the generated Cargo.toml: {err}"),
        )]
    })?;
    std::fs::write(out_dir.join("src").join("main.rs"), &generated.main_rs).map_err(|err| {
        vec![Diagnostic::blocker(
            "E1008",
            format!("failed to write the generated main.rs: {err}"),
        )]
    })?;

    let route_count = module.blueprint.routes.len();
    println!(
        "Parsed {} route{} and wrote the crate to {}",
        route_count,
        if route_count == 1 { "" } else { "s" },
        out_dir.display()
    );

    let status = std::process::Command::new("cargo")
        .arg("build")
        .arg("--release")
        .arg("--manifest-path")
        .arg(out_dir.join("Cargo.toml"))
        .status()
        .map_err(|err| {
            vec![Diagnostic::blocker(
                "E2009",
                format!("failed to run cargo: {err}"),
            )]
        })?;

    if !status.success() {
        return Err(vec![Diagnostic::blocker(
            "E2009",
            "the generated crate failed to compile; review the cargo output above",
        )
        .located(
            "<generated>",
            1,
            Some("run `rivet build` again after fixing the handler bodies; report the issue if the generated code is at fault"),
        )]);
    }

    println!("Build succeeded.");
    println!(
        "Binary: {}/target/release/{}",
        out_dir.display(),
        generated.package_name
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config;
    use std::fs;

    /// A scratch project directory under the system temp dir.
    fn scratch(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("rivet-build-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create scratch dir");
        dir
    }

    #[test]
    fn gauntlet_blocker_stops_build_without_writing_a_crate() {
        let dir = scratch("storyless");
        let app = dir.join("app.py");
        fs::write(
            &app,
            "from rivet import api\n\n@api.get(\"/ping\")\ndef ping() -> dict:\n    return {\"status\": \"pong\"}\n",
        )
        .expect("write app.py");
        let diagnostics = run_build(&app).expect_err("storyless route must fail");
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].error_code, "E2045");
        assert!(diagnostics[0].suggested_fix.is_some());
        assert!(!dir.join("generated").exists(), "no crate may be written");
        fs::remove_dir_all(&dir).expect("clean up scratch dir");
    }

    #[test]
    fn dead_code_warns_through_the_gate_with_default_config() {
        // A helper nothing calls is a warning, not a blocker, so the build
        // gate must not fail on it. Driving run_build past the gate would
        // compile the generated crate, so assert the gate decision instead.
        let dir = scratch("deadcode");
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
        fs::remove_dir_all(&dir).expect("clean up scratch dir");
    }
}
