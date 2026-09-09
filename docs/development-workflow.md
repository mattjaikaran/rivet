# Development Workflow

```mermaid
flowchart LR
    A[Write Python/TS DSL] --> B[Run `rivet build`]
    B --> C{Gauntlet Pass?}
    C -->|Yes| D[Generate Rust Code]
    D --> E[Compile to Binary]
    E --> F[Run]
    C -->|No| G[Print Agentic Error JSON]
    G --> H[AI Fixes Code]
    H --> B
```

## Security & Compliance
- RBAC enforced at compile time (zero runtime overhead).
- CVE Scanning via cargo-deny integrated into the audit.
- SSO & OAuth2 available via plugins.
- SQL Injection Prevention: sqlx compile-time query checking.
