//! Back ends: a [`ServiceBlueprint`] to runnable artifacts.
//!
//! The Rust back end renders a complete, buildable axum crate into the
//! project's `generated/` directory. Later back ends (WASM, UniFFI SDKs)
//! plug in here.

pub mod rust;
