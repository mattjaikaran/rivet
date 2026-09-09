# Phase 0: The Transpiler Spike

**Goal**: Prove that a CLI can read `app.py`, parse `@api` decorators, and
generate a working Rust server.

**Status**: Complete. The spike lives in `examples/basic/` and runs through
`rivet build examples/basic/app.py`.

## Success criteria

- [x] `rivet build` compiles without errors.
- [x] `curl localhost:3000/ping` returns `{"status":"pong"}`.
- [x] `rivet build` writes a compilable crate into `generated/`.

## Acceptance tests

1. Run the spike app:

   ```bash
   cargo run -p rivet-cli -- build examples/basic/app.py
   ./examples/basic/generated/target/release/basic
   ```

2. Send a request:

   ```bash
   curl http://localhost:3000/ping
   ```

   Expected output:

   ```json
   {"status":"pong"}
   ```

3. Send a request with a body:

   ```bash
   curl -X POST http://localhost:3000/echo \
     -H 'Content-Type: application/json' \
     -d '{"hello":"world"}'
   ```

   Expected output:

   ```json
   {"echo":{"hello":"world"}}
   ```

## Scope note

Phase 0 transpiles the documented subset: typed handler signatures, DTO
classes, and handler bodies that return literals, request values, or a single
DTO constructor. Unsupported constructs fail the build with a structured JSON
error; they are not silently mistranslated.
