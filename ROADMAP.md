# SDWANLite Roadmap

Multi-session development plan. Each session picks up from here.

## ✅ Done (v1.0–v2.2.0)

- [x] L4 TCP + L7 HTTP load balancer (round-robin/least-conn/random)
- [x] TLS termination + hot-reload acceptors
- [x] HTTP + TCP health checks (configurable path)
- [x] WebSocket passthrough
- [x] Connection limits + rejected counter
- [x] Runtime backend add/remove via API
- [x] WireGuard mesh (kernel WG + boringtun handshake)
- [x] BGP speaker (capabilities, route refresh, local-pref, multipath, RR)
- [x] ACME HTTP-01 + DNS-01 (Cloudflare/DigitalOcean) + wildcard
- [x] Prometheus /metrics + SSE /api/events
- [x] Firewall rules (config + LB enforcement)
- [x] QoS bandwidth limits (config type)
- [x] Alert event log (ring buffer)
- [x] Docker + compose + systemd + GHCR
- [x] Device CRUD + config apply/versioning
- [x] Controller + edge agent model
- [x] React web UI replacing Dioxus/WASM
- [x] Topology, Diagnostics, Path Labels, Policies, BGP UI
- [x] Bearer auth + loopback default + `--enable-live-actions`

## 🔲 v2.0.0 — flexiWAN-inspired features

### Firewall UI
- [ ] Firewall rules table in React (view/add/toggle/delete)
- [ ] Firewall rule enforcement in LB accept loop

### QoS
- [ ] Token bucket bandwidth limiter per pool (bytes/s)
- [ ] QoS settings UI (per-pool bandwidth cap)

### WAN Failover
- [x] Auto-switch to healthy backend when primary goes down
- [x] Failover notification (alert + UI indicator)

### Samples / Testing
- [x] Import-ready sample payloads for devices, telemetry, firewall, and LB

### Tunnel Management
- [ ] Peer CRUD via REST API (add/remove/list at runtime)
- [ ] Tunnel status dashboard (handshake age, bytes, state)

## 🔮 Future (v2.x+)

- [ ] smoltcp TCP forwarding completion (boringtun data plane)
- [ ] DNS-01 provider abstraction (Route53, Google DNS)
- [ ] HTTP/2 client-facing (h2 server)
- [ ] Multi-node management (central controller)
- [ ] BGP route reflection + communities
- [ ] NAT traversal (STUN/TURN)
- [ ] Application identification (DPI)
- [ ] DHCP server

## Architecture Notes

- Crates: core, sdwan-core, sdwan-agent, lb, mesh, bgp, acme, app
- Frontend: React + Vite + TypeScript (`web-ui/`)
- CI: GitHub Actions (test + release + docker)
