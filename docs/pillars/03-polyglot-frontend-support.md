# 3. Polyglot Frontend Support

Rivet serves the API and the frontend from one origin. `rivet dev` runs the
development topology, and the production build is embedded in the binary.

## Development: `rivet dev`

`rivet dev [app.py]` builds the backend, starts it on a private port, and
serves one origin on the project's configured port.

```bash
rivet dev                                   # the project's own port
rivet dev --frontend-port 5174              # override the detected port
rivet dev --backend-port 3100               # override the private port
```

It detects the frontend from its config file:

| Config file | Framework | Default dev port |
| :--- | :--- | :--- |
| `vite.config.{js,mjs,cjs,ts,mts,cts}` | vite | 5173 |
| `rsbuild.config.*` | rsbuild | 3000 |
| `next.config.*` | next | 3000 |
| `webpack.config.*` | webpack | 8080 |

Start the frontend dev server yourself; `rivet dev` proxies to it, it does
not start it.

### Where each path goes

| Path | Upstream |
| :--- | :--- |
| A route the blueprint declares, such as `/ping` | Rust backend |
| `/api/*` | Rust backend, with the `/api` prefix removed |
| Everything else | Frontend dev server |

A declared route wins over the prefix, so a blueprint route named
`/api/orders` reaches the backend verbatim. With no frontend config, the
backend owns every path. With no declared routes and no frontend, `/api/*`
is the only way to reach the backend.

The generated backend mounts the blueprint's own paths and nothing else.
`/api` is the proxy's escape hatch for backend paths outside the blueprint,
and the proxy strips it before the request goes upstream.

### The HMR tunnel

A dev server's HMR client connects to the page origin, which is the proxy
port, and asks to upgrade the connection. An HTTP client cannot carry that
handshake, so the proxy tunnels it: it replays the request head on a raw
connection to the frontend, relays the `101`, and copies bytes in both
directions until either side closes. A frontend that refuses the upgrade
relays its own answer, with the proxy's body length replacing the
`content-length` the frontend sent.

### Failure behavior

- An upstream that does not answer returns `502` and names the upstream.
- An upstream that does not answer within 30 seconds returns `502`; the
  proxy never hangs.
- The frontend port may not take the proxy port; the clash is `E3018`.
  `rivet dev` reports the port and the `--frontend-port` flag that fixes it.

`rivet dev` stops the backend when it exits or when you press Ctrl-C.

## Production: embedded assets

The built binary carries the frontend inside it, with no sidecar files.
Point `rivet.toml` at the production build and `rivet build` compiles that
directory into the crate it generates:

```toml
[frontend]
dist = "dist"   # the production build directory
spa = true      # serve index.html for a client-side route
```

```bash
rivet build app.py
# Parsed 2 routes and wrote the crate to generated
# Embedding static assets from dist
# Binary: generated/target/release/my-api
```

The keys live in the `[frontend]` section rather than a separate `[assets]`
table: `rivet dev` already owns the frontend concept, and this pillar owns
both halves of it — the dev server behind the proxy, and the production
build inside the binary.

The embedded directory is sealed: a request path that resolves outside it
returns `404`, and a request path with no matching asset returns `404` too,
unless the single-page rule accepts it. Renaming `dist/` after the build
changes nothing, because the binary reads the assets from its own memory.

### What each asset request returns

| Request | Response |
| :--- | :--- |
| A path an embedded file matches | The file, with its guessed content type |
| `/` or a directory path | That directory's own `index.html` |
| A path that names no file, from a client that accepts `text/html` | The embedded `index.html`, when `spa = true` |
| Anything else | `404` |

The single-page rule matches the frontend dev server's own rewrite: the
request is a `GET` or `HEAD`, the path's last segment holds no `.`, and the
`Accept` header names `text/html`. A browser navigation therefore reaches
the client-side router, while an `XHR` to a path the API does not serve
keeps its `404` instead of receiving an HTML page. This matters under
`rivet dev`, whose backend runs the same generated binary.

Every asset response carries an `ETag` computed from the file's SHA-256
hash, and `Cache-Control: public, max-age=0, must-revalidate`. A request
that returns the `ETag` in `If-None-Match` receives `304` and no body. A
`HEAD` receives the file's length and no body.

A blueprint route always wins: the assets mount as the router's fallback,
so `/ping` reaches the handler even when an asset path would match it. When
`dist` names a directory that does not exist, `rivet build` reports
`E2004` as a warning, embeds nothing, and still compiles the crate. A
project with no `[frontend]` section embeds nothing.

The generated router compresses every response with `tower-http`'s
`CompressionLayer`, with the Brotli feature enabled: a client that sends
`Accept-Encoding: br` receives the asset compressed. Compression on the
wire costs no binary size, so the embedded assets stay uncompressed in the
binary; `rust-embed`'s own `compression` feature would trade binary size
instead, and this phase does not measure that trade.

## The admin panel

`[admin] enabled = true` mounts two read-only endpoints on the generated
app:

```toml
[admin]
enabled = true
```

| Request | Response |
| :--- | :--- |
| `GET /__rivet/routes` | The route table as JSON: `method`, `path`, `handler`, and the route's story IDs |
| `GET /__rivet/` | A single-file HTML panel that renders that table |

`rivet build` renders the table from the blueprint, so the endpoint answers
one static string the compiler put in the binary: no serialization and no
state. Both paths carry the `__rivet` prefix, so a blueprint route cannot
collide with them, and neither response is cached.

The panel is one embedded HTML file with no dependencies: it fetches the
table and renders it. Rivet has no Node toolchain in its build, so the
project ships the panel as one static file instead of promising a React or
Solid build step — there is nothing to install and nothing to bundle.

The panel is a developer convenience on a running service, not an
authentication boundary: a project that does not want it exposed leaves
`enabled` false, and a project that serves it in public puts it behind a
plugin or a reverse proxy.
