# 8. WASM & Mobile SDK Export
- WASM: `rivet build --target wasm` compiles to `wasm32-wasi`. Runs on Cloudflare Workers, Fermyon Spin, or Wasmtime.
- Mobile: Using `uniffi`, we generate:
    - Kotlin (Android)
    - Swift (iOS)
    - TypeScript (React Native)

The mobile SDKs contain the same business logic as the backend—no duplication.