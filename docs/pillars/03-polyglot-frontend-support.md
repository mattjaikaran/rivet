# 3. Polyglot Frontend Support

Rivet serves the API and the frontend from one origin. `rivet dev` runs the
development topology; a later phase embeds the production build.

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

A later phase embeds the `dist/` folder into the Rust binary with
`rust-embed` and Brotli compression, so the binary serves static assets
from memory with no sidecar files.
