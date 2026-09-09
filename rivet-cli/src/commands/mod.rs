//! Command implementations for the Rivet CLI.
//!
//! Each subcommand (`build`, `audit`, `history`, `session`, `explain`, ...)
//! lives in its own module and returns structured [`Diagnostic`]s instead of
//! raw errors, so the CLI output stays machine-readable. Context commands
//! (`history`, `session`) read and write the per-project store in
//! [`crate::store`].

pub mod audit;
pub mod build;
pub mod explain;
pub mod history;
pub mod session;
