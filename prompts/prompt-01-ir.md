# Prompt 01: Define the Intermediate Representation (IR)

**Objective**: Create the core data structures that unify Python and TypeScript parsing into a single, strongly-typed Rust model.

**Context**: Every language parser we write (Python, TS, and future languages) must output this exact IR. This allows the rest of the pipeline (Gauntlet, Generator) to be language-agnostic.

---

## Tasks

### 1. Create `rivet-core/src/ir.rs`

Define the following structs and enums with `serde::{Serialize, Deserialize}`.

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Delete,
    Patch,
    Options,
    Head,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FieldDefinition {
    pub name: String,
    pub type_hint: String,        // e.g., "int", "str", "OrderCreate"
    pub is_optional: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StructDefinition {
    pub name: String,
    pub fields: Vec<FieldDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RouteDefinition {
    pub path: String,
    pub method: HttpMethod,
    pub handler_name: String,
    pub request_dto: Option<StructDefinition>,
    pub response_dto: Option<StructDefinition>,
    pub stories: Vec<String>,     // e.g., ["US-123", "EPIC-456"]
    pub middlewares: Vec<String>, // e.g., ["auth", "rate-limit"]
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ServiceBlueprint {
    pub name: String,
    pub routes: Vec<RouteDefinition>,
    pub dependencies: Vec<String>, // e.g., ["UserRepository", "CacheService"]
}
```

### 2. Add Docstrings

Add `///` docstrings to every struct and enum explaining its purpose.

For example:
```rust
/// Represents the HTTP method for a route.
/// Used to map decorators like `@api.get` and `@api.post`.
pub enum HttpMethod { ... }
```

### 3. Implement Display for HttpMethod

```rust
impl std::fmt::Display for HttpMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HttpMethod::Get => write!(f, "GET"),
            HttpMethod::Post => write!(f, "POST"),
            // ... etc
        }
    }
}
```

### 4. Write a Unit Test

In `rivet-core/src/lib.rs`, add:

```rust
#[cfg(test)]
mod tests {
    use super::ir::*;

    #[test]
    fn test_ir_serialization() {
        let blueprint = ServiceBlueprint {
            name: "test".to_string(),
            routes: vec![
                RouteDefinition {
                    path: "/ping".to_string(),
                    method: HttpMethod::Get,
                    handler_name: "ping".to_string(),
                    request_dto: None,
                    response_dto: Some(StructDefinition {
                        name: "Pong".to_string(),
                        fields: vec![
                            FieldDefinition {
                                name: "status".to_string(),
                                type_hint: "String".to_string(),
                                is_optional: false,
                            }
                        ],
                    }),
                    stories: vec!["US-123".to_string()],
                    middlewares: vec![],
                }
            ],
            dependencies: vec![],
        };

        let json = serde_json::to_string(&blueprint).unwrap();
        let deserialized: ServiceBlueprint = serde_json::from_str(&json).unwrap();
        assert_eq!(blueprint, deserialized);
    }
}
```

---

## Acceptance Criteria

- [ ] `cargo test --package rivet-core` passes.
- [ ] The IR structs compile without errors.
- [ ] `serde_json` can serialize and deserialize `ServiceBlueprint`.

---

## Agent Instructions

1. Create `rivet-core/src/ir.rs` and paste the definitions above.
2. Update `rivet-core/src/lib.rs` to export the module: `pub mod ir;`.
3. Add the `Display` implementation for `HttpMethod`.
4. Add the unit test inside `rivet-core/src/lib.rs`.

---

## Output

A PR with the IR module and a passing test.