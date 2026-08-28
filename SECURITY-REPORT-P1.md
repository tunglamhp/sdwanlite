# SECURITY-REPORT-P1 - sdwanlite

Phạm vi: `A:/web/sdwan/sdwanlite` theo `A:/web/sdwan/AGENTS.md` + `A:/web/sdwan/SECURITY-REPORT.md`.
Chỉ audit/verify, không fix thêm.

## Tổng

- CRITICAL: 0
- HIGH: 0
- MEDIUM: 0
- LOW: 1
- Trạng thái hiện tại: P1-1 giữ mức **RESIDUAL**; còn 1 finding LOW.
- Verified: `cargo test --test api_auth` = 5/5 PASS.

## Findings

| Mã | Mô tả | Mức | Trạng thái | File:Line | Ghi chú |
|---|---|---|---|---|---|
| P1-1 | Dashboard fallback trả `include_str!("dashboard.html")`, không có auth riêng; mở UI khi đang bật dev mode. | Low | Mở / RESIDUAL | `crates/app/src/server.rs:180` | UI là control-plane; hiện đã có `auth_middleware` bao toàn bộ router (`crates/app/src/main.rs:140`). Nếu tách dashboard ra sau này phải thêm gate. |

## Đã xác minh

- `SDWANLITE_AUTH_USER`/`SDWANLITE_AUTH_PASS` gate toàn bộ API + dashboard.
- So sánh bearer/Basic dùng `ct_eq()` constant-time.
- Không có secret hardcode trong repo đang audit.
- Service bind loopback trong code/unit tests; non-loopback yêu cầu auth env (`lib.rs:7`).

## Kiểm chử

- `cargo test --test api_auth` = 5 passed, 0 failed.
- `cargo check -p sdwanlite-app` = compile sạch.

## Trở ngại / Lưu ý

- `crates/app/tests/api_auth_mutations.rs` hiện **không biên dịch** với `axum 0.7` (`Router::oneshot` đã thay đổi). Đây là blocker nội bộ của test artifact, không phải finding mới về runtime behavior.
