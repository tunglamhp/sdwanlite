# web-ui — SDWANLite control plane dashboard

React + TypeScript dashboard for the SDWANLite controller API (`/api/v1/*`,
`/stream/config`). Built with Vite; linted with Oxlint; tested with Vitest.

## Commands

```bash
npm install        # install dependencies
npm run dev        # dev server (default http://localhost:5173)
npm run lint       # oxlint
npm run build      # tsc -b && vite build → dist/
npx vitest run     # unit tests
```

## Configuration

- `VITE_API_BASE` — API base URL (default: same origin; set to the controller
  address in dev, e.g. `http://127.0.0.1:8080`).
- Auth token — entered in **Settings**, kept in `sessionStorage` for the
  session, sent as `Authorization: Bearer <token>`.

## Design

Ops-console principles — see `docs/REVIEW-UI.md`: semantic color only, tables
for data, explicit empty/error states, keyboard-first, no external requests
(system font stack), responsive sidebar.
