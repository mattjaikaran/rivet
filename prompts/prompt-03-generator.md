# Prompt 03: The Rust Code Generator (Transpiler)

**Objective**: Take the IR and generate a working Rust `main.rs` that leverages **zero-copy deserialization**, **RAII connections**, **Send+Sync safety**, and **Typestate RBAC**.

**Context**: We use `axum`. The generated code must load `rivet.toml` to enable these Rust-native superpowers.

---

## Tasks

### 1. Create the Generator Module

Create `rivet-cli/src/transpiler/mod.rs` and `rivet-cli/src/transpiler/rust.rs`.

In `mod.rs`, export the Rust generator:
```rust
pub mod rust;
2. Define the Config Struct (for rivet.toml)
Inside rivet-cli/src/transpiler/rust.rs (or a dedicated config.rs), define the Config structure so we can read the features:

rust
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct RustNativeFeatures {
    pub zero_copy_deserialization: bool,
    pub raii_connections: bool,
    pub compile_time_rbac: bool,
    pub const_generics: bool,
}

#[derive(Debug, Deserialize)]
pub struct Config {
    pub rust_native_features: RustNativeFeatures,
}
3. Implement the Generators
A. generate_struct (Zero-Copy & Lifetimes)

Update the struct generator to produce #[serde(borrow)] and lifetime parameters:

rust
fn generate_struct(struct_def: &StructDefinition) -> String {
    let lifetime = if let Some(l) = &struct_def.lifetime_param {
        format!("<{}>", l) // e.g., OrderCreate<'a>
    } else {
        String::new()
    };

    let mut s = format!(
        "#[derive(Debug, Serialize, Deserialize)]\npub struct {}{} {{\n",
        struct_def.name, lifetime
    );

    for field in &struct_def.fields {
        let optional = if field.is_optional { "Option<" } else { "" };
        let close = if field.is_optional { ">" } else { "" };
        let borrowed_attr = if field.is_borrowed {
            r#"#[serde(borrow)] "#
        } else {
            ""
        };
        s.push_str(&format!(
            "    {}{}: {}{}{},\n",
            borrowed_attr, field.name, optional, field.type_hint, close
        ));
    }
    s.push_str("}\n\n");
    s
}
B. generate_handler (RAII, Send+Sync, Typestate)

The generated handler must enforce the new rules:

rust
fn generate_handler(route: &RouteDefinition, config: &Config) -> String {
    let handler_name = &route.handler_name;
    let is_protected = route.stories.iter().any(|s| s.contains("admin") || s.contains("protected"));

    // Typestate: If protected, require AuthenticatedRequest
    let request_param = if let Some(dto) = &route.request_dto {
        if is_protected && config.rust_native_features.compile_time_rbac {
            format!("req: AuthenticatedRequest, Json(payload): Json<{}>", dto.name)
        } else {
            format!("Json(payload): Json<{}>", dto.name)
        }
    } else {
        if is_protected && config.rust_native_features.compile_time_rbac {
            "req: AuthenticatedRequest".to_string()
        } else {
            "()".to_string()
        }
    };

    let response_type = if let Some(dto) = &route.response_dto {
        format!("JsonResponse<{}>", dto.name)
    } else {
        "JsonResponse<serde_json::Value>".to_string()
    };

    format!(
        r#"
async fn {handler_name}(
    {request_param}
) -> {response_type} {{
    // RAII: Any DB connection acquired here auto-releases on function exit.
    // Send+Sync: If the compiler fails here, wrap state in Arc<tokio::sync::Mutex>.
    let response = serde_json::json!({{
        "status": "ok",
        "message": "Handler '{handler_name}' implemented."
    }});
    JsonResponse(response)
}}
"#,
        handler_name = handler_name,
        request_param = request_param,
        response_type = response_type,
    )
}
C. generate_router (Typestate Layer)

Wrap the router with the RequireAuth middleware if RBAC is enabled:

rust
fn generate_router(blueprint: &ServiceBlueprint, config: &Config) -> String {
    let mut router = "let app = Router::new()\n".to_string();
    for route in &blueprint.routes {
        let method_str = match route.method {
            HttpMethod::Get => ".route(\"/{route_path}\", get({handler_name}))\n",
            HttpMethod::Post => ".route(\"/{route_path}\", post({handler_name}))\n",
            HttpMethod::Put => ".route(\"/{route_path}\", put({handler_name}))\n",
            HttpMethod::Delete => ".route(\"/{route_path}\", delete({handler_name}))\n",
            HttpMethod::Patch => ".route(\"/{route_path}\", patch({handler_name}))\n",
            _ => ".route(\"/{route_path}\", get({handler_name}))\n",
        };
        router.push_str(&format!(
            method_str,
            route_path = route.path.trim_start_matches('/'),
            handler_name = route.handler_name
        ));
    }

    if config.rust_native_features.compile_time_rbac {
        router.push_str(r#".layer(RequireAuth::new()) // Typestate: Unauthenticated -> Authenticated"#);
    }

    format!(
        r#"
#[tokio::main]
async fn main() {{
    tracing_subscriber::fmt::init();
    {}
}}
"#,
        router
    )
}
4. Update run_build to Load Config
In rivet-cli/src/commands/build.rs, load rivet.toml and pass it to the generator:

rust
use crate::parser::python::parse_python_file;
use crate::transpiler::rust::{generate_rust_code, Config};
use std::fs;
use std::path::Path;

pub fn run_build() -> anyhow::Result<()> {
    // Load the configuration
    let config_contents = fs::read_to_string("rivet.toml")?;
    let config: Config = toml::from_str(&config_contents)?;

    let app_py = Path::new("app.py");
    if !app_py.exists() {
        anyhow::bail!("app.py not found. Run `rivet new` to create one.");
    }

    let blueprint = parse_python_file(app_py)?;
    let rust_code = generate_rust_code(&blueprint, &config)?;

    fs::create_dir_all("generated/src")?;
    fs::write("generated/src/main.rs", rust_code)?;

    let cargo_toml = r#"
[package]
name = "app"
version = "0.1.0"
edition = "2021"

[dependencies]
axum = "0.7"
tokio = { version = "1.40", features = ["full"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
tracing = "0.1"
tracing-subscriber = "0.3"
"#;
    fs::write("generated/Cargo.toml", cargo_toml)?;

    let status = std::process::Command::new("cargo")
        .arg("build")
        .arg("--manifest-path")
        .arg("generated/Cargo.toml")
        .arg("--release")
        .status()?;

    if status.success() {
        println!("✅ Transpilation successful!");
        println!("   Binary: ./generated/target/release/app");
    } else {
        anyhow::bail!("❌ Cargo build failed. Check the generated code in ./generated/");
    }

    Ok(())
}
Acceptance Criteria
□ generate_struct produces #[serde(borrow)] for borrowed fields.
□ generate_handler enforces AuthenticatedRequest for protected routes.
□ run_build successfully reads rivet.toml and passes the config.
□ cargo build on the generated code passes.
Agent Instructions
Add toml = "0.8" to rivet-cli/Cargo.toml dependencies.

Create rivet-cli/src/transpiler/mod.rs and rivet-cli/src/transpiler/rust.rs.

Copy the code above, ensuring the Config struct and generator functions are fully implemented.

Create rivet-cli/src/commands/build.rs with the updated run_build.

Update main.rs to call commands::build::run_build().

Output
A PR where rivet build reads rivet.toml, transpiles app.py with zero-copy and RBAC, and compiles a working Rust binary.


---

### 3. Verify Your Dependencies

Before the agent starts coding, ensure your `rivet-cli/Cargo.toml` has these exact entries (copy this if needed):

```toml
[dependencies]
rivet-core = { path = "../rivet-core" }
tokio = { version = "1.40", features = ["full"] }
axum = "0.7"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
tree-sitter = "0.22"
tree-sitter-python = "0.20"
clap = { version = "4.5", features = ["derive", "env"] }
anyhow = "1.0"
thiserror = "1.0"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
toml = "0.8"  # <-- THIS MUST BE HERE FOR PROMPT 03