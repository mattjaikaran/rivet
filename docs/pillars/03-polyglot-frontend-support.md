# 3. Polyglot Frontend Support
The dev server (`rivet dev`) auto-detects Vite, Rsbuild, Webpack, or Next.js.

- Dev: Proxies /api to the Rust backend, everything else to the frontend HMR server.
- Prod: Embeds the `dist/` folder into the Rust binary (`rust-embed` with Brotli compression). Static assets are served from memory—sub-millisecond responses.