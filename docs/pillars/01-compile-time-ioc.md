# 1. Compile-Time IoC (Dependency Injection)
Using Rust's type system, we resolve dependencies at compile time with `#[injectable]` macros.

```rust
#[injectable]
struct UserService {
    #[inject]
    repository: Arc<dyn UserRepository>,
    #[inject(env("MAX_RETRIES"))]
    max_retries: u32,
}
```

Benefits:
- Zero runtime lookups (no hashmap, no reflection).
- Circular dependencies cause compiler errors.
- Scoped containers (request-scoped, session-scoped).
