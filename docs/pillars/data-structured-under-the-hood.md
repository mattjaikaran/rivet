# Data Structures Used (Under the Hood)
Data Structure | Rust Crate | Purpose
| :--- | :--- | :--- |
Merkle Tree | `SHA256` hashing | Test caching (skip unchanged routes)
Directed Acyclic Graph (DAG) | `petgraph` | Service dependency resolution
Radix Trie | `matchit` | Ultra-fast routing (O(n) matching)
LRU Cache | `lru` | Query result caching
Priority Queue | `std::collections::BinaryHeap` | Background job prioritization
Bloom Filter | `bloom` | Idempotency checks (avoid storing millions of IDs)
Vector Index | `LanceDB` (ANN) | Semantic code search for agents