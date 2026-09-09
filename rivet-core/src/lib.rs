//! Core library for Rivet.
//!
//! This crate holds the intermediate representation (IR) that every language
//! front end emits and every code generator consumes. Keeping the IR in its
//! own crate lets front ends and back ends evolve independently and keeps the
//! contract serializable for tooling and tests.

pub mod ir;
