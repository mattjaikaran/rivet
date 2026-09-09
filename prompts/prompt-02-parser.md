# Prompt 02: Implement the Python DSL Parser

**Objective**: Parse a Python file (`app.py`) using `tree-sitter` and extract the IR, including the new Rust-native hints.

**Strictness**: If a function lacks type hints, the parser must return an error `E1001` (Type Safety Violation).

---

## Tasks

### 1. Create the Parser Module

Create `rivet-cli/src/parser/mod.rs` and `rivet-cli/src/parser/python.rs`.

In `mod.rs`, export the Python parser:
```rust
pub mod python;

2. Implement parse_python_file
In python.rs, implement the main entry point:

```rust
use anyhow::{Context, Result};
use std::path::Path;
use tree_sitter::Parser;
use tree_sitter_python::language as python_language;
use rivet_core::ir::ServiceBlueprint;

pub fn parse_python_file(path: &Path) -> Result<ServiceBlueprint> {
    let source = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read file: {:?}", path))?;

    let mut parser = Parser::new();
    parser.set_language(&python_language())
        .context("Failed to set Python language for tree-sitter")?;

    let tree = parser.parse(&source, None)
        .context("Failed to parse Python file")?;

    let root = tree.root_node();

    let mut routes = Vec::new();
    traverse_node(&root, &source, &mut routes)?;

    Ok(ServiceBlueprint {
        name: path.file_stem().unwrap_or_default().to_string_lossy().to_string(),
        routes,
        dependencies: vec![],
    })
}
```
3. Traverse the AST
Write a recursive function traverse_node that walks the AST, finds function_definition nodes with an @api decorator, and extracts the route data.

Crucial Upgrade (Rust Native Features):
While extracting field type hints, you must detect these special Python annotations and map them to the IR fields:

Python DSL Syntax	IR Field Value	Resulting Rust Code
borrowed: str	is_borrowed = true	#[serde(borrow)] field: &'a str
List[float, 768]	array_size = Some(768)	field: [f64; 768]
List[int, 512]	array_size = Some(512)	field: [i64; 512]
Implementation Snippet for the Type Parser:

```rust
fn parse_type_hint(type_node: &Node, source: &str, field_name: &str) -> FieldDefinition {
    let type_text = type_node.utf8_text(source).unwrap_or("Unknown");
    let mut is_borrowed = false;
    let mut array_size = None;
    let mut type_name = type_text.to_string();

    // Detect borrowing (e.g., "borrowed: str")
    if type_text.contains("borrowed") {
        is_borrowed = true;
        type_name = "&str".to_string();
    }

    // Detect const generics (e.g., "List[float, 768]")
    if type_text.starts_with("List[") && type_text.ends_with("]") {
        let inner = &type_text[5..type_text.len()-1];
        let parts: Vec<&str> = inner.split(',').map(|s| s.trim()).collect();
        if parts.len() == 2 {
            if let Ok(size) = parts[1].parse::<usize>() {
                array_size = Some(size);
                let base_type = match parts[0] {
                    "float" => "f64",
                    "int" => "i64",
                    _ => "serde_json::Value",
                };
                type_name = format!("[{}; {}]", base_type, size);
            }
        }
    }

    FieldDefinition {
        name: field_name.to_string(),
        type_hint: type_name,
        is_optional: false, // Detect Optional[...] later
        is_borrowed,
        array_size,
    }
}
```
4. Error Handling
If a function has no type hints, return a custom error:

```rust
#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("Missing type hint for function '{function_name}'")]
    MissingTypeHint { function_name: String },
    #[error("Invalid decorator syntax on line {line}")]
    InvalidDecorator { line: usize },
}
```
5. Unit Test
Write a test that parses a raw Python string with the new syntax:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_borrowed_and_array_syntax() {
        let source = r#"
from rivet import api

@api.post("/order")
def create_order(request: OrderCreate) -> OrderResponse:
    pass
"#;
        let blueprint = parse_python_string(source).unwrap();
        // Assert that fields are correctly parsed
    }
}
```

Acceptance Criteria
□ parse_python_file("app.py") returns a valid ServiceBlueprint.
□ A field borrowed: str sets is_borrowed = true.
□ A field List[float, 768] sets array_size = Some(768).
□ A function without type hints returns a MissingTypeHint error.
Agent Instructions
Add tree-sitter and tree-sitter-python to rivet-cli/Cargo.toml.

Add thiserror for error handling.

Create rivet-cli/src/parser/mod.rs and rivet-cli/src/parser/python.rs.

Implement the full traversal logic as described.

Write the unit tests.

Output
A PR with a working Python parser that can extract route definitions and Rust-native type hints from app.py.