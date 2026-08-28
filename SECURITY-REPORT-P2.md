# SECURITY-REPORT-P2 — sdwanlite

**Auditor:** Pocock-Security-Auditor
**Date:** 2026-08-28
**Scope:** toàn bộ `sdwanlite/` dựa trên `AGENTS.md`, `SECURITY-REPORT-P0.md`, `SECURITY-REPORT-P1.md`.
**Chế độ:** audit/verify only. Không fix thêm trong báo cáo này.

---

## Tổng

- CRITICAL: 0
- HIGH: 1
- MEDIUM: 2
- LOW: 3
- INFO: 2

Trạng thái hiện tại: **1 HIGH, 2 MEDIUM, 3 LOW, 2 INFO** cần theo dõi/xử lý; phần còn lại là **RESIDUAL** có lý do chấp nhận rủi ro có kiểm soát.

## Findings

| Mã | Mô tả | Mức | Trạng thái | File:Line | Ghi chú |
|---|---|---|---|---|---|
| P2-1 | Auth chiến lược bị phân tán: dashboard dùng Basic, API dùng Bearer, controller edge dùng bootstrap-token riêng. Rủi ro xác thực không nhất quán, khó rotate/revoke tập trung. | High | Cần xử lý | `crates/app/src/server.rs:103-140`, `crates/sdwan-agent/src/controller.rs:182-193`, `PRODUCTION.md:65-86` | P1/P2 nên thống nhất một lớp auth chính; hiện đang chấp nhận do phạm vi P0. |
| P2-2 | Thiếu TLS ở tầng controller/data-plane nội bộ: HTTP/WS nếu expose ngoài loopback hoặc qua mạng nội bộ thì sniff được token và config. | Medium | Cần xử lý | `docs/ARCHITECTURE-P0.md:129-137`, `docker-compose.yml:16-35` | P1 nên thêm rustls/reverse proxy; hiện chỉ loopback. |
| P2-3 | `api_auth_mutations.rs` không biên dịch với axum 0.7 (`Router::oneshot` đã thay đổi). Làm mất confidence vào auth test artifact và khó CI hóa. | Medium | Đang mở | `crates/app/tests/api_auth_mutations.rs` | Chặn nội bộ, không phải runtime bug, nhưng cần fix để CI xanh. |
| P2-4 | `auth_middleware` bỏ qua `WWW-Authenticate` khi sai credential ở một số nhánh; browser không nhắc lại Basic. | Low | Cần xử lý | `crates/app/src/server.rs:132-138` | Hiện có header, nhưng cần đồng nhất toàn bộ unauthorized path. |
| P2-5 | `/metrics`, `/healthz` không auth; khi bind non-loopback có thể lộ số liệu/trạng thái. | Low | RESIDUAL | `crates/app/src/server.rs:154-155`, `crates/sdwan-agent/src/controller.rs:207-221` | Chấp nhận trong P0/P1 vì docs nói internal trust zone; cần gate khi có TLS/non-loopback. |
| P2-6 | `REPLACE_WITH_REAL_TOKEN` vẫn có trong repo config mẫu; dễ để nhầm commit thật. | Low | Cần xử lý | `sdwanlite.toml:7`, `crates/app/src/main.rs:32-34` | Hiện có startup warning; có thể thay bằng sentinel rỗng hoặc fail fast. |
| P2-7 | CI thiếu SAST/SCA/lint gate ngoài `cargo audit`. Không chặn secrets/code-smell trước merge. | Info | Cần thêm | `.github/workflows/ci.yml:15-20` | Thêm `cargo deny`, `cargo clippy --all-targets -D warnings`, `gitleaks`. |
| P2-8 | `bootstrap_token` xuất hiện ở `--bootstrap-token` argv; có thể lộ qua `ps`. | Info | RESIDUAL | `crates/sdwan-agent/src/main.rs:74-80`, `crates/sdwan-agent/src/main.rs:140-163` | P0 đã khuyến nghị `--bootstrap-token-file`; cần log warning/block non-loopback khi dùng argv path. |

## Đã xác minh

- Không có secret thật trong repo đang audit; chỉ có placeholder và mã test.
- `ct_eq()` dùng cho Bearer/Basic so sánh trong `server.rs` và `controller.rs`.
- Loopback binding mặc định trong `main.rs`/controller `run_controller`; non-loopback cần cờ rõ ràng trong docs.
- Token file 0600 được kiểm tra ở agent startup (`read_token`) trên Unix.
- `/healthz` + `/metrics` được document là internal trust zone.
- Web UI hiện là placeholder; không có nhập liệu nhạy cảm trong UI này.

## Kiểm chử

- Đã đọc toàn bộ auth path: `server.rs`, `controller.rs`, `agent.rs`, `main.rs`.
- Đã grep toàn repo với các mẫu: `auth_middleware|SDWANLITE_AUTH_|api_token|bootstrap_token|Authorization|check_auth|authorized\(`, `password\s*=|secret\s*=|token\s*=|REPLACE_WITH_REAL_TOKEN|-----BEGIN|api_token`, `Command::new|std::process::Command|unwrap\(\)|expect\(|fs::write|tokio::fs::write|0600|0644|chmod|sudo|root`.
- Đã kiểm tra `web-ui/src/api.ts`: không tự gắn Authorization; phụ thuộc env/tùy chọn tích hợp sau.
- Đã kiểm tra `docker-compose.yml`: không mount secret file; chỉ mount config ro.
