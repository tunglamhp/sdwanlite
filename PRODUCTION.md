# Production deployment guide

Three supported deployment shapes, all using existing tooling (kernel
WireGuard, Caddy, systemd, Docker) — no custom data-plane code required.

## 1. Bare metal / VM (systemd)

```bash
sudo install -m 0755 target/release/sdwanlited /usr/local/bin/
sudo mkdir -p /etc/sdwanlite
sudo cp sdwanlite.toml /etc/sdwanlite/
sudo useradd -r -s /usr/sbin/nologin sdwanlite || true
sudo chown -R sdwanlite:sdwanlite /etc/sdwanlite
sudo install -m 0644 deploy/systemd/sdwanlite.service /etc/systemd/system/
sudo systemctl daemon-reload && sudo systemctl enable --now sdwanlite
```

Mesh: create `/etc/wireguard/sdwanlite0.conf` (the mesh module renders one via
`GET /api/mesh/keypair` + `wg-quick`) and enable `wg-quick@sdwanlite0`; the
unit starts after it.

## 2. Docker Compose

```bash
docker compose up -d
# with off-the-shelf HTTP/2 edge (Caddy terminates HTTPS/h2, proxies to us):
docker compose --profile edge up -d
```

The image ships `wireguard-tools`; mount `/dev/net/tun`, add `NET_ADMIN`
(uncommented in compose) and the mesh module works unchanged inside the
container using kernel WireGuard.

## 3. Client-facing HTTP/2

Do not hand-roll it. Put Caddy (or any mature h2 edge) on :443 and point its
reverse_proxy at an sdwanlite HTTP pool — see `Caddyfile`. ACME for the edge
hostname is handled by Caddy; wildcard/internal certs stay on sdwanlite's own
TLS pools.

## 4. Observability

```bash
docker compose --profile monitoring up -d
```

- Prometheus scrapes `sdwanlite:8080/metrics` every 15 s.
- Grafana at `:3000` (anonymous viewer) with a pre-provisioned SDWANLite
  dashboard: backend health/connections/bytes, pool rejections, BGP RIB +
  established sessions, uptime.

## Hardening checklist

- Set `general.api_token` before exposing the API beyond loopback.
- TLS pools: real cert/key files (ACME output or your own CA).
- Backends: use private network addresses reachable only from the node.
- Keep `PermitRootLogin no` on the host; the appliance default warning in the
  README applies to upstream, not to this distribution's packaging.
