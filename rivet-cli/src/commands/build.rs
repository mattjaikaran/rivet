//! `rivet build`: parse the DSL entry point, generate the Rust crate, and
//! compile it with cargo.

use crate::config::RivetConfig;
use crate::diagnostic::Diagnostic;
use crate::parser::python::parse_python_file;
use crate::transpiler::rust::generate_project;
use std::path::Path;

pub fn run_build(app_file: &Path) -> Result<(), Diagnostic> {
    let project_dir = app_file
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| ".".into());

    let config =
        RivetConfig::load(&project_dir).map_err(|message| Diagnostic::blocker("E1008", message))?;

    if !app_file.exists() {
        return Err(
            Diagnostic::blocker("E1008", format!("{} not found", app_file.display())).located(
                project_dir.display().to_string(),
                1,
                Some("write a `from rivet import api` module, then run `rivet build`"),
            ),
        );
    }

    let blueprint = parse_python_file(app_file)?;

    let generated = generate_project(&blueprint, &config)?;

    let out_dir = project_dir.join("generated");
    std::fs::create_dir_all(out_dir.join("src")).map_err(|err| {
        Diagnostic::blocker(
            "E1008",
            format!("failed to create the generated directory: {err}"),
        )
    })?;
    std::fs::write(out_dir.join("Cargo.toml"), &generated.cargo_toml).map_err(|err| {
        Diagnostic::blocker(
            "E1008",
            format!("failed to write the generated Cargo.toml: {err}"),
        )
    })?;
    std::fs::write(out_dir.join("src").join("main.rs"), &generated.main_rs).map_err(|err| {
        Diagnostic::blocker(
            "E1008",
            format!("failed to write the generated main.rs: {err}"),
        )
    })?;

    let route_count = blueprint.routes.len();
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
        .map_err(|err| Diagnostic::blocker("E2009", format!("failed to run cargo: {err}")))?;

    if !status.success() {
        return Err(Diagnostic::blocker(
            "E2009",
            "the generated crate failed to compile; review the cargo output above",
        )
        .located("<generated>", 1, Some("run `rivet build` again after fixing the handler bodies; report the issue if the generated code is at fault")));
    }

    println!("Build succeeded.");
    println!(
        "Binary: {}/target/release/{}",
        out_dir.display(),
        generated.package_name
    );
    Ok(())
}
