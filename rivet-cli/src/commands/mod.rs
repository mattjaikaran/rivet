//! Command implementations for the Rivet CLI.
//!
//! Each subcommand (`build`, later `dev`, `audit`, `session`, ...) lives in
//! its own module and returns structured [`Diagnostic`]s instead of raw
//! errors, so the CLI output stays machine-readable.

pub mod build;
