//! Rivet CLI: transpile a Python (later TypeScript) DSL into a fast Rust
//! server.
//!
//! Every command reports failures as structured, machine-readable JSON on
//! stderr (see [`diagnostic::Diagnostic`]) so agents and humans see the same
//! error. A command may return several diagnostics at once — the Gauntlet
//! emits one per finding — so the CLI prints every line before failing.
//!
//! Every invocation is recorded in the project context store before it runs
//! (see [`store`]): `rivet history` lists what ran, `rivet session` saves and
//! resumes the context around it, and `rivet explain` traces a symptom to its
//! introducing commit.
//!
//! Agents type slash commands (`rivet /plan "..."`); main() strips the
//! leading `/` before clap parses so the subcommand names stay plain.

#![allow(clippy::result_large_err)] // Diagnostics are self-contained JSON payloads

mod commands;
mod config;
mod diagnostic;
mod gauntlet;
mod mcp;
mod parser;
mod plugin;
mod store;
mod transpiler;

#[cfg(test)]
mod test_support;

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
    /// Record a project dependency in `rivet.toml` (phase 4).
    Add {
        #[command(subcommand)]
        action: AddAction,
    },
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
    /// List recorded commands with their exit status and timing.
    History {
        /// Path to the app module that owns the store (defaults to
        /// `app.py`; the store lives next to it).
        #[arg(default_value = "app.py")]
        app: PathBuf,
    },
    /// Trace a symptom to the route and commit most likely to have
    /// introduced it.
    Explain {
        /// The symptom to search for, for example "orders endpoint 500".
        symptom: String,
        /// Path to the app module that owns the store (defaults to
        /// `app.py`; the store lives next to it).
        #[arg(default_value = "app.py")]
        app: PathBuf,
    },
    /// Plan a user story into a working branch with passing tests,
    /// backed by an LLM provider or a prepared module (phase 3).
    Plan {
        /// The user story, for example "add referral codes US-42".
        story: String,
        /// Path to the app module (defaults to `app.py`).
        #[arg(default_value = "app.py")]
        app: PathBuf,
        /// Apply this prepared replacement module instead of calling the
        /// provider (deterministic path used by CI and tests).
        #[arg(long)]
        from: Option<PathBuf>,
        /// Push and open a pull request with `gh` after the plan commits.
        #[arg(long)]
        push: bool,
    },
    /// Auto-fix what the Gauntlet can deterministically repair (phase 3).
    Fix {
        /// Path to the app module (defaults to `app.py`).
        #[arg(default_value = "app.py")]
        app: PathBuf,
    },
    /// Trace a symptom through the module, the generated code, and git.
    Trace {
        /// The symptom to trace, for example "orders endpoint 500".
        symptom: String,
        /// Path to the app module (defaults to `app.py`).
        #[arg(default_value = "app.py")]
        app: PathBuf,
    },
    /// Serve the Model Context Protocol (MCP) server over stdio so AI
    /// agents can call Rivet tools (phase 3, pillar 09).
    Mcp,
    /// Save, list, or resume a compact markdown session context.
    Session {
        #[command(subcommand)]
        action: SessionAction,
    },
}

#[derive(Debug, Subcommand)]
enum AddAction {
    /// Add or update a plugin entry. The generated app composes the plugin
    /// at compile time (phase 4, pillar 01).
    Plugin {
        /// Plugin name, for example `auth-token`.
        name: String,
        /// Directory of the plugin crate, relative to the project.
        #[arg(long)]
        path: Option<String>,
        /// Plugin crate name; defaults to `rivet-plugin-<name>`.
        #[arg(long = "crate")]
        crate_name: Option<String>,
        /// Registry version, used when `--path` is absent.
        #[arg(long)]
        version: Option<String>,
        /// Path to the app module (defaults to `app.py`).
        #[arg(default_value = "app.py")]
        app: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
enum SessionAction {
    /// Save the current module context as a named session.
    Save {
        /// Session name, for example `debug-orders`.
        name: String,
        /// Path to the app module that owns the store (defaults to
        /// `app.py`; the store lives next to it).
        #[arg(default_value = "app.py")]
        app: PathBuf,
    },
    /// Print a saved session context so work can continue.
    Resume {
        /// The session name `save` created.
        name: String,
        /// Path to the app module that owns the store (defaults to
        /// `app.py`; the store lives next to it).
        #[arg(default_value = "app.py")]
        app: PathBuf,
    },
    /// List saved session names.
    List {
        /// Path to the app module that owns the store (defaults to
        /// `app.py`; the store lives next to it).
        #[arg(default_value = "app.py")]
        app: PathBuf,
    },
}

impl Command {
    /// Short command label recorded in the store, for example `build` or
    /// `session save`.
    fn label(&self) -> String {
        match self {
            Command::Add { .. } => "add plugin".into(),
            Command::Build { .. } => "build".into(),
            Command::Audit { .. } => "audit".into(),
            Command::History { .. } => "history".into(),
            Command::Explain { .. } => "explain".into(),
            Command::Plan { .. } => "plan".into(),
            Command::Fix { .. } => "fix".into(),
            Command::Trace { .. } => "trace".into(),
            Command::Mcp => "mcp".into(),
            Command::Session { action, .. } => match action {
                SessionAction::Save { .. } => "session save".into(),
                SessionAction::Resume { .. } => "session resume".into(),
                SessionAction::List { .. } => "session list".into(),
            },
        }
    }

    /// The app path that anchors the project store.
    fn app_path(&self) -> PathBuf {
        match self {
            Command::Add { action } => match action {
                AddAction::Plugin { app, .. } => app.clone(),
            },
            Command::Build { app } | Command::Audit { app, .. } | Command::History { app } => {
                app.clone()
            }
            Command::Explain { app, .. } => app.clone(),
            Command::Plan { app, .. } | Command::Fix { app } | Command::Trace { app, .. } => {
                app.clone()
            }
            Command::Mcp => PathBuf::new(),
            Command::Session { action } => match action {
                SessionAction::Save { app, .. }
                | SessionAction::Resume { app, .. }
                | SessionAction::List { app } => app.clone(),
            },
        }
    }
}

fn main() -> ExitCode {
    // Agents type slash commands: `rivet /plan "..."`. Strip the leading
    // "/" from the subcommand token so clap sees `plan`, `fix`, `trace`.
    let args: Vec<String> = std::env::args().collect();
    let normalized: Vec<String> = args
        .iter()
        .enumerate()
        .map(|(index, arg)| {
            if index == 1 && arg.starts_with('/') {
                arg.trim_start_matches('/').to_string()
            } else {
                arg.clone()
            }
        })
        .collect();
    let cli = Cli::parse_from(normalized);
    let label = cli.command.label();
    let app = cli.command.app_path();
    let project_dir = store::project_dir_for(&app);

    // Record the invocation before it runs; a store problem is a warning,
    // never a reason to fail the command itself.
    let recording = match store::open(&project_dir) {
        Ok(conn) => store::start_command(&conn, &label)
            .ok()
            .map(|id| (conn, id)),
        Err(diagnostic) => {
            eprintln!("{}", diagnostic.summary());
            None
        }
    };
    let started = std::time::Instant::now();

    let result = match cli.command {
        Command::Add { action } => match action {
            AddAction::Plugin {
                name,
                path,
                crate_name,
                version,
                app,
            } => commands::add::run_add_plugin(
                &app,
                &name,
                path.as_deref(),
                crate_name.as_deref(),
                version.as_deref(),
            ),
        },
        Command::Build { app } => commands::build::run_build(&app),
        Command::Audit { app, json } => commands::audit::run_audit(&app, json),
        Command::History { app } => commands::history::run_history(&app),
        Command::Explain { symptom, app } => commands::explain::run_explain(&symptom, &app),
        Command::Plan {
            story,
            app,
            from,
            push,
        } => commands::plan::run_plan(&story, &app, from.as_deref(), push),
        Command::Fix { app } => commands::fix::run_fix(&app),
        Command::Trace { symptom, app } => commands::trace::run_trace(&symptom, &app),
        Command::Mcp => mcp::run_stdio().map_err(|diagnostic| vec![diagnostic]),
        Command::Session { action } => match action {
            SessionAction::Save { name, app } => commands::session::run_session_save(&app, &name),
            SessionAction::Resume { name, app } => {
                commands::session::run_session_resume(&app, &name)
            }
            SessionAction::List { app } => commands::session::run_session_list(&app),
        },
    };

    if let Some((conn, id)) = recording {
        let status = if result.is_ok() { 0 } else { 1 };
        let _ = store::finish_command(&conn, id, status, started.elapsed());
    }

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
