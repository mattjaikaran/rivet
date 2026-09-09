# 4. Context Engine (SQLite + LanceDB)
- SQLite stores:
    - Command history (`rivet history`)
    - Session logs (`rivet session save`)
    - AST fingerprints per commit
    - GitHub Issues/PRs (synced via CLI)
- LanceDB (vector DB) stores:
    - Code embeddings for semantic search
    - Similarity between functions, commits, and user stories
