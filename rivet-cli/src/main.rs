//! Rivet CLI: transpile a Python (later TypeScript) DSL into a fast Rust
//! server.
//!
//! Every command reports failures as structured, machine-readable JSON on
//! stderr (see [`diagnostic::Diagnostic`]) so agents and humans see the same
//! error. A command may return several diagnostics at once — the Gauntlet
//! emits one per finding — so the CLI prints every line before failing.

#![allow(clippy::result_large_err)] // Diagnostics are self-contained JSON payloads

mod commands;
mod config;
mod diagnostic;
mod gauntlet;
mod parser;
mod transpiler;

use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::process::ExitCode;

/// Rivet: write your API in Python DSL, run it as a compiled Rust binary.
#[derive(Debug, Parser)]
#[command(name = "rivet", version, about, arg_required_else_help = true)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Parse the app entry point and compile the generated Rust crate.
    Build {
        /// Path to the app module (defaults to `app.py`).
        #[arg(default_value = "app.py")]
        app: PathBuf,
    },
    /// Run the Gauntlet and report the phase-1 MQI grade (pillar 06).
    Audit {
        /// Path to the app module (defaults to `app.py`).
        #[arg(default_value = "app.py")]
        app: PathBuf,
        /// Print only the JSON breakdown.
        #[arg(long)]
        json: bool,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Build { app } => commands::build::run_build(&app),
        Command::Audit { app, json } => commands::audit::run_audit(&app, json),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(diagnostics) => {
            for diagnostic in diagnostics {
                eprintln!("{}", diagnostic.to_json());
                eprintln!("{}", diagnostic.summary());
            }
            ExitCode::FAILURE
        }
    }
}
