## Testing Strategy
- Unit tests: Each route and service is tested in isolation.
- Integration tests: The full transpilation pipeline is tested end-to-end.
- Mutation tests: `cargo-mutants` ensures tests catch edge cases.
- Performance benchmarks: `criterion` tracks latency and throughput.

```text

---

```markdown
# Phase 0: The Transpiler Spike

**Goal**: Prove that we can parse a Python DSL and generate a working Rust HTTP server.

**Duration**: 2 Weeks

---

## Success Criteria

- [ ] The CLI can parse `@api.get("/ping")` from `app.py`.
- [ ] The CLI generates a `main.rs` with a `/ping` route.
- [ ] `cargo build` on the generated code passes without errors.
- [ ] The binary responds to `curl localhost:3000/ping` with a 200 OK and JSON body.
```


## Acceptance Tests

1. Create a new project:
   ```bash
   rivet new test-app
   cd test-app
```

2. write `app.py`:
```python
from rivet import api

@api.get("/ping")
def ping() -> dict:
    return {"status": "pong"}
    ```
3. Run the transpiler:
```bash
rivet build
```
4. Run the generated binary:
```bash
./generated/target/release/test-app
```
5. Send a request:
```bash
curl http://localhost:3000/ping
```
*Expected output*:
```json
{"status": "pong"}
```