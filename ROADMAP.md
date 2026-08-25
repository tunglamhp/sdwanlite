# SDWANLite Roadmap

Multi-session development plan. Each session picks up from here.

## ✅ Done (v1.0–v1.5)

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
- [x] Dioxus/WASM dashboard (sidebar, light/dark, topology drag/zoom)
- [x] Firewall rules (config + LB enforcement)
- [x] QoS bandwidth limits (config type)
- [x] Alert event log (ring buffer)
- [x] Docker + compose + systemd + GHCR

## 🔲 v2.0.0 — flexiWAN-inspired features

### Firewall UI
- [ ] Firewall rules table in Dioxus (view/add/toggle/delete)
- [ ] Firewall rule enforcement in LB accept loop

### QoS
- [ ] Token bucket bandwidth limiter per pool (bytes/s)
- [ ] QoS settings UI (per-pool bandwidth cap)

### WAN Failover
- [ ] Auto-switch to healthy backend when primary goes down
- [ ] Failover notification (alert + UI indicator)

### Alerts
- [ ] Alert feed UI (real-time event list)
- [ ] Webhook stub (POST to external URL on alert)

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

- 8 crates: core, lb, mesh, bgp, acme, app, web, + tests
- ~5,500 lines Rust, 23 tests, 0 warnings
- Frontend: Dioxus 0.7 WASM (crates/web, excluded from workspace)
- Build: `dx build --platform web` → `web-dist/`
- CI: GitHub Actions (test + release + docker)
