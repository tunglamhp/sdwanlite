# Prompt: Redesign sdwanlite Dashboard — MikroTik SD-WAN Style

## Context

You are working on `sdwanlite`, an open-source SD-WAN edge appliance written in Rust.
Repo: current working directory. All code is original (MIT license).

**Stack**: Rust + tokio + axum (backend API), Dioxus 0.7 WASM (frontend), boringtun (WG data plane), smoltcp (userspace TCP/IP).

**Frontend crate**: `crates/web/` (excluded from workspace, built with `dx build --platform web`).
**CSS**: inlined in `crates/web/index.html` inside `<style>` tags (NOT a separate file — dx does not copy assets).
**Output**: `dx build --platform web --release` → copy `target/dx/*/release/web/public/` → `web-dist/` at repo root.
**Serving**: axum serves `web-dist/` via ServeDir; falls back to legacy embedded HTML if missing.

## Design Reference: MikroTik SD-WAN (mikrotiksdwan.com)

Study the MikroTik SD-WAN cloud dashboard design and apply these patterns:

### Layout
- **Left sidebar** (fixed, ~230px): logo at top, nav items with icons, LIVE indicator + theme toggle at bottom
- **Main content area**: full-width content with breadcrumb-style page title
- **Responsive**: sidebar collapses on mobile

### Color Palette — LIGHT GREEN (xanh lá cây nhạt)
- **Sidebar**: dark forest green `#1a2e1a`, active item `#2e5a2e`, hover `#243d24`
- **Sidebar text**: soft sage `#a8c8a8`, active `#ffffff`
- **Main background**: very light sage `#f4f7f4` (light mode) or `#0a100a` (dark mode)
- **Cards**: white `#ffffff` with subtle green border `#d4e0d4`
- **Primary green**: `#2e7d32` (buttons, active states, links)
- **Accent green**: `#4caf50` (highlights, hover)
- **Success**: `#2e7d32`, **Error**: `#d32f2f`, **Warning**: `#f9a825`
- **Text**: dark forest `#1a2e1a` (light mode), `#c8e8c8` (dark mode)
- **Muted text**: `#6b856b`
- **Status pills**: green bg `#e8f5e9`, red bg `#fce4ec`, amber bg `#fff8e1`

### Typography
- Font: "Segoe UI", system-ui, sans-serif
- Headings: 700 weight, 20-22px
- Labels: 10-11px, uppercase, letter-spacing 1-1.5px, weight 600
- Body: 13-13.5px
- Mono: Consolas/"Cascadia Code" for addresses, keys, ports

### Components (MikroTik style)
1. **KPI cards**: white bg, green left-border accent (3px), icon at top, large value, small label
2. **Status pills**: rounded, colored bg (green=ok, red=down, amber=warn, blue=info)
3. **Data tables**: clean, minimal borders, hover row highlight, uppercase column headers
4. **Topology**: SVG with draggable nodes, zoom/pan, edge labels, color-coded by status
5. **Sidebar nav**: icons + labels, active state with left border accent + bg change
6. **Action buttons**: outlined, green accent, rounded corners
7. **Toast notifications**: bottom-right, slide-in animation

### MikroTik SD-WAN specific UI patterns to adopt
- **Device-centric view**: show device/node status prominently with health indicators
- **Tunnel visualization**: graphical representation of tunnels (solid = connected, dashed = disconnected)
- **Quick stats row**: 4-6 KPI cards at top (uptime, peers, tunnels, throughput)
- **Section-based navigation**: Dashboard > Devices > Tunnels > Firewall > QoS > Alerts > Logs
- **Status-first design**: every element shows its current state (color-coded)
- **Compact tables**: dense information with small font, clear column separation

## Current Features (MUST preserve all)

| Tab | Features |
|---|---|
| Overview | KPI cards (node/uptime/pools/RIB), BGP sessions table, System status |
| Topology | Interactive SVG: drag nodes, wheel zoom, pan, vertical/horizontal/radial layouts, fit/reset, positions persisted |
| Mesh | WG status, keypair generator, live peers table (RX/TX/handshake) |
| BGP | Sessions table (neighbor/AS/state/prefixes/flaps), RIB entries + sparkline |
| Load Balancers | TCP pools (health/active/total/rejected per backend), HTTP routes |
| Actions | Customizable buttons (add/remove/execute), toast feedback, localStorage persistence |
| Firewall | Rules table (action/port/protocol/source/comment) |
| Alerts | Event log feed (severity/source/message/timestamp) |
| QoS | Per-pool bandwidth summary (algorithm/active/rejected/backends/health) |

## API Endpoints

- `GET /api/status` → { node, version, uptime_secs, mesh_enabled, mesh_peers_configured, bgp_enabled, bgp_sessions, bgp_rib_size, lb }
- `GET /api/lb` → { tcp: [...], http: [...] }
- `GET /api/mesh/status` → { available, peers: [...] }
- `GET /api/mesh/keypair` → { private_key, public_key }
- `GET /api/bgp/rib` → { count, routes: [...] }
- `GET /api/alerts` → { count, events: [...] }
- `GET /api/firewall` → { rules: [...] }
- `POST /api/reload` — reload config
- `POST /api/tls/reload` — rebuild TLS acceptors

## Technical Constraints

- Dioxus 0.7 rsx: use `for x in iter { element {} }` (NOT `.map().collect()`)
- Signals: `let mut sig = use_signal(|| initial);` read with `sig()`, write with `sig.set()`
- Resource: `let data = use_resource(|| async { fetch... });` read with `data.read().as_ref()`
- Match pattern: `match data.read().as_ref() { Some(Ok(d)) => ..., Some(Err(e)) => ..., None => ... }`
- CSS: inline in index.html `<style>` tag (dx does NOT copy separate CSS files)
- WASM: no tokio; use `gloo_timers::future::TimeoutFuture::new(ms).await` for delays
- Events: `onclick`, `oninput`, `onwheel` (`e.delta().delta_y`), `onpointerdown/move/up` (`e.client_coordinates()`)
- localStorage: `web_sys::window()?.local_storage().ok()??` for get, `.set_item()` for set

## What to Build

Redesign `crates/web/assets/main.css` and `crates/web/src/main.rs` to create a professional,
MikroTik SD-WAN inspired dashboard with a light green color scheme. The UI should look like
a commercial network management console — clean, modern, information-dense but not cluttered.

Then:
1. `dx build --platform web --release`
2. Copy `target/dx/*/release/web/public/` → `web-dist/`
3. Restart daemon
4. Verify in browser at http://127.0.0.1:18080
