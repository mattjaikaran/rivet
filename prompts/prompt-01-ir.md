# Prompt 01: Define the Intermediate Representation (IR)

**Objective**: Create the core data structures that unify Python and TypeScript parsing.

**Context**: Every language parser we write must output this exact IR. This allows the rest of the pipeline (Gauntlet, Generator) to leverage Rust's zero-copy and const-generic features.

## Tasks

### 1. Create `rivet-core/src/ir.rs`

Define the following structs and enums. **Note the new fields** for borrowing and arrays:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum HttpMethod {
    Get, Post, Put, Delete, Patch, Options, Head,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FieldDefinition {
    pub name: String,
    pub type_hint: String,        // e.g., "int", "&str", "[f64; 768]"
    pub is_optional: bool,
    // NEW: For zero-copy deserialization (maps Python 'borrowed: str' to Rust '&str')
    pub is_borrowed: bool,
    // NEW: For const generics (maps Python 'List[float, 768]' to Rust '[f64; 768]')
    pub array_size: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StructDefinition {
    pub name: String,
    pub fields: Vec<FieldDefinition>,
    // NEW: For zero-copy structs (e.g., OrderCreate<'a>)
    pub lifetime_param: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RouteDefinition {
    pub path: String,
    pub method: HttpMethod,
    pub handler_name: String,
    pub request_dto: Option<StructDefinition>,
    pub response_dto: Option<StructDefinition>,
    pub stories: Vec<String>,
    pub middlewares: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ServiceBlueprint {
    pub name: String,
    pub routes: Vec<RouteDefinition>,
    pub dependencies: Vec<String>,
}