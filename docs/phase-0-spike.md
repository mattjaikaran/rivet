# Phase 0: The Transpiler Spike

**Goal**: Build a CLI that reads `app.py`, parses `@api.post`, and generates a working Rust server.

**Success Criteria**:
- [ ] `rivet build` compiles without errors.
- [ ] `curl localhost:3000/ping` returns `{"status":"pong"}`.

**Acceptance Tests**:
1. Create `app.py`:
   ```python
   from rivet import api

   @api.get("/ping")
   def ping() -> dict:
       return {"status": "pong"}
