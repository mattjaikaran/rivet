# Rivet Architecture

This document explains the high-level design, data flow, and internal components of Rivet.

---

## Overview

Rivet is a **polyglot transpiler** with a **Rust runtime**. It ingests Python/TypeScript DSL, validates it against strict architectural "Verifiers", and outputs optimized Rust code that compiles to a native binary or WASM.

The core philosophy: **"If it compiles, it's correct, secure, and performant."**

---

## The Transpilation Pipeline

```mermaid
flowchart TD
    A[Python/TS DSL] --> B[tree-sitter Parser]
    B --> C[Intermediate Representation (IR)]
    C --> D[The Verifier Linter]
    D -->|Pass| E[Rust Code Generator]
    D -->|Fail| F[Agentic Error JSON]
    E --> G[axum/hyper Server]
    G --> H[Native Binary / WASM]