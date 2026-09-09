# Rivet · The Structural API Framework

[![License: MIT/Apache-2.0](https://img.shields.io/badge/License-MIT%2FApache--2.0-blue.svg)](LICENSE)
[![MQI](https://img.shields.io/badge/MQI-A%2B-brightgreen)]()
[![Built with Rust](https://img.shields.io/badge/Built%20with-Rust-orange)](https://www.rust-lang.org/)
[![PRs Welcome](https://img.shields.io/badge/PRs-welcome-brightgreen.svg)](CONTRIBUTING.md)

**Rivet** is a next-generation API framework that combines the ergonomics of Python (Django/FastAPI) and TypeScript (NestJS) with the raw speed, memory safety, and concurrency of Rust.

**It is built for the AI era.** You write business logic in a Python/TS DSL. The **Rivet CLI** parses your code, enforces strict architectural "Gauntlets" (complexity < 8, 0 dead code, 0 surviving mutants), and transpiles it to a blazing-fast, multi-threaded Rust binary.

---

## Why Rivet?

| Feature | Description |
| :--- | :--- |
| 🦀 **Rust Runtime, Polyglot Surface** | Zero-cost abstractions, no GIL, true parallelism. Write in Python/TS, run as Rust. |
| 🤖 **Agent-Native** | Built-in MCP (Model Context Protocol) server and a local Vector DB (LanceDB) so AI agents understand your codebase semantically. |
| 🏗️ **Compile-Time Architecture** | Hexagonal architecture, IoC, and RBAC are enforced at compile time—not runtime. If it compiles, it's secure. |
| 🧪 **The Gauntlet** | Enforces Cyclomatic Complexity < 8, 0 `any`/`unknown` types, 0 duplicate code, and 100% mutation survival. No AI slop. |
| 📱 **Mobile Ready** | Generate native Kotlin (Android), Swift (iOS), and React Native SDKs from your Rust core via UniFFI. |
| 🌊 **WASM First** | Compile your entire API to `wasm32-wasi` for sub-millisecond cold starts on Cloudflare Workers or Fermyon Spin. |
| 🧠 **Super CLI** | Slash-commands for agents: `/plan`, `/fix`, `/trace`. Live TUI dashboard with request logging. |
| 🔒 **Strict by Default** | Zero `any`/`unknown` types. Type safety is enforced at the DSL level. Configurable to be less strict, but default is rigid. |

---

## Quick Start (Vision)

```bash
# Install the CLI
cargo install rivet-cli

# Create a new project
rivet new my-api --arch multi-service

# Write a route in Python DSL (app.py)
@api.post("/orders", stories=["US-123"])
def create_order(request: OrderCreate) -> OrderResponse:
    # Your business logic here
    return OrderResponse(status="ok")

# Run the Gauntlet & transpile to Rust
rivet build --release

# Run the native binary
./target/release/my-api
