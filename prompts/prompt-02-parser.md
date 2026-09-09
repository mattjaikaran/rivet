# Prompt 02: Implement the Python DSL Parser

**Objective**: Parse a Python file (`app.py`) using `tree-sitter` and extract the IR.

**Context**: We use `tree-sitter-python` to parse the file into a syntax tree, then traverse it to find `@api` decorators.

**Strictness**: If a function lacks type hints, the parser must return an error `E1001` (Type Safety Violation).

---

## Tasks

### 1. Create the Parser Module

Create `rivet-cli/src/parser/mod.rs` and `rivet-cli/src/parser/python.rs`.

In `mod.rs`, export the Python parser:
```rust
pub mod python;
```

### 2. Implement `parse_python_file`

In `python.rs`, implement:

```rust
use anyhow::{Context, Result};
use std::path::Path;
use tree_sitter::{Language, Parser, Node};
use tree_sitter_python::language as python_language;
use rivet_core::ir::{ServiceBlueprint, RouteDefinition, HttpMethod, StructDefinition, FieldDefinition};

pub fn parse_python_file(path: &Path) -> Result<ServiceBlueprint> {
    let source = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read file: {:?}", path))?;

    let mut parser = Parser::new();
    parser.set_language(&python_language())
        .context("Failed to set Python language for tree-sitter")?;

    let tree = parser.parse(&source, None)
        .context("Failed to parse Python file")?;

    let root = tree.root_node();

    // Traverse the AST to find function definitions with decorators
    let mut routes = Vec::new();
    traverse_node(&root, &source, &mut routes)?;

    Ok(ServiceBlueprint {
        name: path.file_stem().unwrap_or_default().to_string_lossy().to_string(),
        routes,
        dependencies: vec![], // We'll populate this in Phase 1
    })
}
```

### 3. Traverse the AST

Write a recursive function `traverse_node` that:

- Walks the AST.
- Finds `function_definition` nodes.
- Checks if the function has a `decorator` list.
- Looks for decorators named `api.get`, `api.post`, `api.put`, etc.
- Extracts:
  - The function name (from the `identifier` child).
  - The path string (from the decorator's argument, e.g., `"/orders"`).
  - The request type hint (from the first parameter's type annotation).
  - The response type hint (from the `->` return type annotation).
  - The `stories` list (from the decorator keyword arguments).

**Example of extracting a decorator:**

```rust
fn extract_route_from_decorator(
    decorator_node: &Node,
    source: &str,
    function_node: &Node,
) -> Option<RouteDefinition> {
    // Find the call node inside the decorator
    let mut call_node = None;
    let mut cursor = decorator_node.walk();
    for child in decorator_node.children(&mut cursor) {
        if child.kind() == "call" {
            call_node = Some(child);
            break;
        }
    }

    let call = call_node?;
    // The call has a "identifier" and an "argument_list"
    let mut method = None;
    let mut path = None;
    let mut stories = Vec::new();

    // ... parse the call node ...
    // This is where you extract method (get/post) and path ("/ping")
    // And extract stories from keyword arguments

    Some(RouteDefinition {
        path: path?,
        method: method?,
        handler_name: function_node.child_by_field_name("name")?.utf8_text(source)?.to_string(),
        request_dto: None, // We'll add this in Phase 1
        response_dto: None,
        stories,
        middlewares: vec![],
    })
}
```

### 4. Error Handling

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

### 5. Unit Test

Write a test that parses a raw Python string:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ping_route() {
        let source = r#"
from rivet import api

@api.get("/ping")
def ping() -> dict:
    return {"status": "pong"}
"#;

        let blueprint = parse_python_string(source).unwrap();
        assert_eq!(blueprint.routes.len(), 1);
        let route = &blueprint.routes[0];
        assert_eq!(route.path, "/ping");
        assert_eq!(route.method, HttpMethod::Get);
        assert_eq!(route.handler_name, "ping");
        assert_eq!(route.stories, Vec::<String>::new());
    }

    #[test]
    fn test_missing_type_hint_errors() {
        let source = r#"
from rivet import api

@api.get("/ping")
def ping():
    return {"status": "pong"}
"#;
        let result = parse_python_string(source);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("Missing type hint"));
    }
}
```

---

## Acceptance Criteria

- [ ] `parse_python_file("app.py")` returns a valid `ServiceBlueprint`.
- [ ] The test with `@api.get("/ping")` passes.
- [ ] A function without type hints returns a `MissingTypeHint` error.

---

## Agent Instructions

1. Add `tree-sitter` and `tree-sitter-python` to `rivet-cli/Cargo.toml`.
2. Create `rivet-cli/src/parser/mod.rs` and `rivet-cli/src/parser/python.rs`.
3. Copy the implementation above, filling in the traversal logic.
4. Use `thiserror` for error handling (add it to `Cargo.toml` if not already).
5. Write the unit tests.

---


## Output

A PR with a working Python parser that can extract a route definition from `app.py`.