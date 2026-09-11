//! `rivet build --target wasm`: write the WASI crate and compile it.
//!
//! The crate shares the native target's DTO structs and service layer, so a
//! route's logic has one implementation. Everything that needs a server is
//! absent: no axum, no tokio, no gRPC, no plugins, and no embedded assets.
//!
//! The build checks the `wasm32-wasip1` target before it runs cargo, so a
//! missing target is `E2010` with the `rustup target add` command that fixes
//! it, not a wall of cargo output.

use crate::config::RivetConfig;
use crate::diagnostic::Diagnostic;
use crate::transpiler::rust::{WASM_TARGET, generate_wasm_project};
use rivet_core::ir::ServiceBlueprint;
use std::path::Path;

/// The directory the WebAssembly crate is written to.
///
/// It sits beside the native target's `generated/`, so building one target
/// never overwrites the other's outputs.
pub(super) const OUT_DIR: &str = "generated-wasm";

/// Write and compile the WebAssembly crate.
pub(super) fn run(
    project_dir: &Path,
    blueprint: &ServiceBlueprint,
    config: &RivetConfig,
) -> Result<(), Vec<Diagnostic>> {
    if !target_installed()? {
        return Err(vec![Diagnostic::blocker(
            "E2010",
            format!("the `{WASM_TARGET}` target is not installed"),
            format!(
                "run `rustup target add {WASM_TARGET}`, then rerun `rivet build --target wasm`"
            ),
        )]);
    }

    let project =
        generate_wasm_project(blueprint, config).map_err(|diagnostic| vec![diagnostic])?;
    let out_dir = project_dir.join(OUT_DIR);
    std::fs::create_dir_all(out_dir.join("src")).map_err(|err| write_error("directory", err))?;
    std::fs::write(out_dir.join("Cargo.toml"), &project.cargo_toml)
        .map_err(|err| write_error("Cargo.toml", err))?;
    std::fs::write(out_dir.join("src").join("main.rs"), &project.main_rs)
        .map_err(|err| write_error("src/main.rs", err))?;

    println!(
        "Parsed {} route{} and wrote the WASI crate to {}",
        blueprint.routes.len(),
        if blueprint.routes.len() == 1 { "" } else { "s" },
        out_dir.display()
    );

    let status = std::process::Command::new("cargo")
        .arg("build")
        .arg("--release")
        .arg("--target")
        .arg(WASM_TARGET)
        .arg("--manifest-path")
        .arg(out_dir.join("Cargo.toml"))
        .status()
        .map_err(|err| {
            vec![Diagnostic::blocker(
                "E2009",
                format!("failed to run cargo: {err}"),
                "make sure `cargo` is installed and on your PATH, then rerun `rivet build --target wasm`",
            )]
        })?;
    if !status.success() {
        return Err(vec![Diagnostic::blocker(
            "E2009",
            "the generated WASI crate failed to compile; review the cargo output above",
            "run `rivet build --target wasm` again after fixing the handler bodies; report the issue if the generated code is at fault",
        )
        .located("<generated-wasm>", 1)]);
    }

    let module = out_dir
        .join("target")
        .join(WASM_TARGET)
        .join("release")
        .join(format!("{}.wasm", project.package_name));
    println!("Build succeeded.");
    println!("Module: {}", module.display());
    println!(
        "Run it with a WASI host, for example a small driver that writes the request JSON\nand calls the exported `rivet_invoke`; see `docs/pillars/08-wasm-mobile-sdk-support.md`."
    );
    Ok(())
}

/// Whether the `wasm32-wasip1` standard library is installed.
fn target_installed() -> Result<bool, Vec<Diagnostic>> {
    let output = std::process::Command::new("rustup")
        .arg("target")
        .arg("list")
        .arg("--installed")
        .output()
        .map_err(|err| {
            vec![Diagnostic::blocker(
                "E2010",
                format!("failed to run rustup: {err}"),
                "install the Rust toolchain with rustup, then rerun `rivet build --target wasm`",
            )]
        })?;
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .any(|line| line.trim() == WASM_TARGET))
}

/// The diagnostic for a failed write into the generated directory.
fn write_error(what: &str, err: std::io::Error) -> Vec<Diagnostic> {
    vec![Diagnostic::blocker(
        "E1008",
        format!("failed to write the generated {what}: {err}"),
        "free disk space or fix write permissions on the project's `generated-wasm` directory, then rerun `rivet build --target wasm`",
    )]
}
