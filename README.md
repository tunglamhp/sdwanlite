# SDWANLite

SD-WAN control plane + edge agent written in Rust, with a React web UI.
Clean-room, learning-oriented implementation. **Version 2.2.0**

## Kiến trúc

| Thành phần | Vai trò |
|---|---|
| `sdwan-agent` | Edge agent **hoặc** controller. Agent báo telemetry & nhận cấu hình qua WS; controller cấp phát thiết bị, lưu cấu hình, sinh alerts. |
| `web-ui` | Dashboard React (Vite + TypeScript): Devices CRUD, config editor (firewall/routes/QoS/path-labels), Topology, Diagnostics, Path Labels, Policies, BGP. |
| Crates Rust | `core`, `sdwan-core`, `lb` (load balancer), `mesh` (WireGuard), `bgp`, `acme`, `app` (sdwanlited). |

## Build

```bash
cargo build --release            # sdwan-agent
cd web-ui && npm ci && npm run build
```

## Run

Controller (mặc định bind loopback):

```bash
cargo run -p sdwan-agent -- --mode controller --bind 127.0.0.1:8090 \
  --bootstrap-token <token>
# web UI: http://127.0.0.1:5199  (chạy `web-ui: npm run dev`)
```

Edge agent:

```bash
cargo run -p sdwan-agent -- --controller http://127.0.0.1:8090 \
  --bootstrap-token <token> --device-id <uuid>
```

Kernel-affecting actions chỉ chạy khi có `--enable-live-actions`.

## API (`/api/v1`)

| Endpoint | Mô tả |
|---|---|
| `POST /devices/register` | Đăng ký thiết bị mới |
| `GET /devices` · `GET/PUT/DELETE /devices/:id` | CRUD thiết bị |
| `PUT /devices/:id/config` | Áp cấu hình (firewall/routes/QoS/path-labels) — phiên bản phải tăng |
| `POST /telemetry` | Edge agent gửi telemetry (uptime, links, flags) |
| `GET /alerts` | Danh sách alert (ring buffer, tối đa 100) |

## Thay đổi trong v2.2.0

- **Device CRUD hoàn chỉnh**: register, get/update/delete, áp cấu hình qua API.
- **Alerts**: backend sinh alert khi flag chuyển trạng thái (không spam); hiển thị feed trên Dashboard.
- **Web UI mới** (React thay Dioxus cũ): form Add device, config editor với nút Apply (tự tăng version, có xác nhận "verified"), Topology theo telemetry thật, Diagnostics chi tiết, chart băng thông TX/RX.
- **Bảo mật**: bind loopback mặc định, auth bằng bearer token, `--enable-live-actions` để mở action kernel.
- Kiểm thử: `cargo test` 38/38, `vitest` 17/17, lint + build sạch.

## CI

Push lên `main`/`dev` chạy: `cargo test` + build + audit + web checks. Tag `v*` tự build release binaries (Linux/Windows) và tạo GitHub Release.

## License

MIT
