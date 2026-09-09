# 2. Multi-Service Architecture
Services communicate via strongly-typed internal channels. This allows zero-cost abstraction: the same code can run as a monolith (in-memory) or microservices (gRPC) by changing a single config flag.

```mermaid
flowchart LR
    subgraph Monolith
        S1[Service A]
        S2[Service B]
        S3[Service C]
    end
    S1 <-->|Internal Channels| S2
    S2 <-->|Internal Channels| S3

    subgraph Microservices
        M1[Service A]
        M2[Service B]
        M3[Service C]
    end
    M1 <-->|gRPC/HTTP| M2
    M2 <-->|gRPC/HTTP| M3
```