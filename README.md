# <img src="docs/sdwanlite-logo.svg" alt="SDWANLite logo" width="72" align="top"/> SDWANLite

A clean-room, learning-oriented SD-WAN edge + load balancer written in Rust.
Original implementation — not a fork of any existing appliance.

## Components

| Crate | Purpose |
|---|---|
| `core` | TOML configuration model, shared types |
| `lb` | L4 TCP load balancer (round-robin / least-connections / random) + HTTP/1.1 reverse proxy with host/path routing, health checks, X-Forwarded-For |
| `mesh` | WireGuard mesh control plane: native Curve25519 keypairs, `wg-quick` config rendering, apply/status via `wg` tools (Linux) |
| `bgp` | Minimal RFC 4271 speaker for labs: OPEN/KEEPALIVE/UPDATE/NOTIFICATION, IPv4-unicast NLRI, small RIB |
| `app` (`sdwanlited`) | REST API + embedded dark-theme dashboard |

## Build

```bash
cargo build --release
```

## Run

```bash
./target/release/sdwanlited sdwanlite.toml
# dashboard: http://127.0.0.1:8080/
```

If no config file is found a built-in demo config is used.

### Example config

```toml
[general]
name = "edge-1"
api_port = 8080

[mesh]
enabled = true
private_key = "<base64 key from POST-less GET /api/mesh/keypair>"
listen_port = 51820

[[mesh.peers]]
name = "site-b"
public_key = "<peer public key>"
endpoint = "203.0.113.2:51820"
allowed_ips = ["10.100.0.2/32"]

[bgp]
enabled = true
router_id = "10.100.0.1"
local_as = 65000
networks = ["10.100.0.0/24"]
[[bgp.neighbors]]
ip = "10.100.0.2"
remote_as = 65000

[[lb.tcp]]
name = "web-cluster"
listen = "0.0.0.0:9000"
algorithm = "least_connections"
backends = ["10.100.0.11:80", "10.100.0.12:80"]

[[lb.http]]
name = "api-gateway"
listen = "0.0.0.0:9090"
[[lb.http.routes]]
path_prefix = "/v1/"
backends = ["10.100.0.21:8080"]
```

## API

| Endpoint | Description |
|---|---|
| `GET /` | Web dashboard |
| `GET /api/status` | Node, mesh and BGP summary |
| `GET /api/lb` | Pool/backend health and counters |
| `GET /api/mesh/keypair` | Generate WireGuard keypair |
| `GET /api/bgp/rib` | Learned prefixes |

## Status & roadmap

Working today: L4 + L7 load balancing with live health checks, WG key
generation/config rendering, lab-grade BGP sessions, REST + dashboard.

Known limitations (learning project): BGP has no capabilities negotiation or
policy filters; mesh apply requires Linux + `wg-quick`; HTTP proxy is
HTTP/1.1-only; no TLS termination yet.

## License

MIT
