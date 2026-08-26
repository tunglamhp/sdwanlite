# sdwanlite-web — Dioxus/WASM Dashboard

Frontend control panel for sdwanlite, built with Dioxus 0.7 (WASM).

## Run (dev)

```bash
cd crates/web
dx serve --platform web
# opens at http://127.0.0.1:1030 with hot-reload
```

## Build (production)

```bash
cd crates/web
dx build --platform web --release
# output: target/dx/sdwanlite-web/release/web/public/
```

Copy `public/` → `web-dist/` at repo root, then run `sdwanlited`.

## Environment Variables

| Variable | Description | Default |
|---|---|---|
| `SDWANLITE_AUTH_USER` | HTTP Basic Auth username | *(unset = dev mode, no auth)* |
| `SDWANLITE_AUTH_PASS` | HTTP Basic Auth password | *(unset = dev mode)* |

When auth env vars are set, all `/api/*` routes require
`Authorization: Basic base64(user:pass)`.

## API Endpoints

See `PRODUCTION.md` at repo root for the full list.

## Screenshot

<!-- TODO: add screenshot after visual verification -->
![dashboard](https://via.placeholder.com/800x450?text=SDWANLite+Dashboard)
