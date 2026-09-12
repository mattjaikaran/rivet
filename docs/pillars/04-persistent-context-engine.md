# 4. Persistent Context Engine (SQLite + LanceDB)

The context engine gives Rivet a local, queryable memory of what it has
done, so humans and agents can resume work across sessions.

## Store layout

One store lives per project at `.rivet/` next to the app module, in the
same directory as `app.py` and `rivet.toml`. It is gitignored. The store
holds two back ends:

- `.rivet/rivet.db` - the SQLite store (command history, sessions,
  fingerprints).
- `.rivet/lancedb` - the LanceDB vector index (blueprint chunks).

SQLite and vector access live in `rivet-cli/src/store/` and never in
`rivet-core`, so the core stays WASM-friendly.

## SQLite store

- Crate: `rusqlite` with the `bundled` feature, so the CLI carries its own
  SQLite and compiles on macOS, Linux, and the later WASM CI targets
  without a system dependency.
- Tables:
  - `commands`: every CLI invocation with timestamp, exit status, and
    duration.
  - `sessions`: named markdown context blobs with a created-at time.
  - `fingerprints`: per-commit AST digests for `rivet explain`.
- A command is recorded before it runs with a null status, then its exit
  status and duration are filled in when it returns. A row that stays null
  is an invocation that never finished.
- Schema migration is idempotent (`PRAGMA user_version`): opening a store
  creates and migrates it, so a fresh or stale store converges.

Commands:

- `rivet history [app]` lists recorded commands newest-first with an RFC
  3339 timestamp, exit status, and duration.
- `rivet session save <name> [app]` renders the current module context (the
  parsed blueprint, the `[verifier]` config, and the diagnostics it
  produced) as compact markdown and stores it.
- `rivet session resume <name> [app]` prints a saved session back verbatim.
- `rivet session list [app]` lists the saved session names.

## Vector index

- Crate: `lancedb` (0.38). Even a local store needs the `remote` feature:
  upstream `job.rs` references `Error::Http`, which only exists under that
  feature.
- Blueprints are indexed as chunks, one per route: method, path, handler,
  and story IDs. Each chunk embeds deterministically (character trigrams
  hashed into a fixed-size float vector, no model, no network) into
  `.rivet/lancedb`.
- `rivet explain "<symptom>" [app]` re-indexes the current blueprint,
  embeds the symptom, returns the nearest route chunk, and reports the
  commit that introduced it. The introducing commit comes from git
  pickaxe on the matched handler (`git log -S <handler>`); the module
  digest is recorded against the current commit as a fingerprint.
- The MSRV is 1.91, set by lancedb 0.38.

## Roadmap

Phase-2 checkboxes are ticked in `docs/ROADMAP.md`; finished tracker
lines live in `tasks/completed.md`.
