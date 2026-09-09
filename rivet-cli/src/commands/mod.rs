//! Command implementations for the Rivet CLI.
//!
//! Each subcommand (`build`, `audit`, later `dev`, `session`, ...) lives in
//! its own module and returns structured [`Diagnostic`]s instead of raw
//! errors, so the CLI output stays machine-readable.

pub mod audit;
pub mod build;
