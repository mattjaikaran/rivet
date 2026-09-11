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
use crate::transpiler::rust::{AssetEmbedding, generate_project};
use std::path::Path;

mod wasm;

/// Which artifact `rivet build` produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum BuildTarget {
    /// A native binary: the axum server (the default).
    Native,
    /// A `wasm32-wasip1` core module that answers the same routes.
    Wasm,
}

pub fn run_build(app_file: &Path, target: BuildTarget) -> Result<(), Vec<Diagnostic>> {
    let project_dir = app_file
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| ".".into());

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
                "write a `from rivet import api` module, then run `rivet build`",
            )
            .located(project_dir.display().to_string(), 1),
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

    if target == BuildTarget::Wasm {
        return wasm::run(&project_dir, &module.blueprint, &config);
    }

    let generated = generate_project(&module.blueprint, &config, &project_dir)
        .map_err(|diagnostic| vec![diagnostic])?;

    if let AssetEmbedding::Missing(dir) = &generated.assets {
        let warning = Diagnostic::warning(
            "E2004",
            format!(
                "the frontend build directory {} does not exist, so the binary serves no assets",
                dir.display()
            ),
            "build the frontend into that directory, or point `[frontend] dist` at the directory that holds it, then rerun `rivet build`",
        );
        eprintln!("{}", warning.to_json());
        eprintln!("{}", warning.summary());
    }

    let out_dir = project_dir.join("generated");
    std::fs::create_dir_all(out_dir.join("src")).map_err(|err| {
        vec![Diagnostic::blocker(
            "E1008",
            format!("failed to create the generated directory: {err}"),
            "free disk space or fix write permissions on the project's `generated` directory, then rerun `rivet build`",
        )]
    })?;
    std::fs::write(out_dir.join("Cargo.toml"), &generated.cargo_toml).map_err(|err| {
        vec![Diagnostic::blocker(
            "E1008",
            format!("failed to write the generated Cargo.toml: {err}"),
            "free disk space or fix write permissions on the project's `generated` directory, then rerun `rivet build`",
        )]
    })?;
    std::fs::write(out_dir.join("src").join("main.rs"), &generated.main_rs).map_err(|err| {
        vec![Diagnostic::blocker(
            "E1008",
            format!("failed to write the generated main.rs: {err}"),
            "free disk space or fix write permissions on the project's `generated` directory, then rerun `rivet build`",
        )]
    })?;

    let route_count = module.blueprint.routes.len();
    println!(
        "Parsed {} route{} and wrote the crate to {}",
        route_count,
        if route_count == 1 { "" } else { "s" },
        out_dir.display()
    );
    if let AssetEmbedding::Embedded { dir, .. } = &generated.assets {
        println!("Embedding static assets from {}", dir.display());
    }

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
                "make sure `cargo` is installed and on your PATH, then rerun `rivet build`",
            )]
        })?;

    if !status.success() {
        return Err(vec![Diagnostic::blocker(
            "E2009",
            "the generated crate failed to compile; review the cargo output above",
            "run `rivet build` again after fixing the handler bodies; report the issue if the generated code is at fault",
        )
        .located("<generated>", 1)]);
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
mod tests;
