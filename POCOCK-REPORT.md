# Pocock Team Report — sdwanlite P0

## Team Status

| Agent | Status | Deliverable |
|---|---|---|
| Architect | idle | P0 scaffold: crates `sdwan-core` + `sdwan-agent`, Axum endpoints, SQLite-backed store. |
| Type-Designer | idle | Branded types (`DeviceId`, `OrgId`, `SiteId`), device lifecycle state machine, schemars derives. |
| Tester | idle | proptest for config validation + state machine transitions; insta snapshots for API responses. |
| Security-Auditor | idle | Reviewed `store.rs` rewrite; findings tracked in `SECURITY-REPORT-P1.md`. |
| Code-Reviewer | idle | Reviewing P1 diffs; output in `REVIEW-P1.md`. |

## P1 Fixes Applied — `sdwan-agent`

| Issue | Fix in `crates/sdwan-agent/src/...` | Result |
|---|---|---|
| Mismatched broadcast sender types | `store.rs`: use `Arc<tokio::sync::broadcast::Sender<DeviceConfig>>`; wrap clones in `Arc::new(...)` | compile OK |
| Await outside async blocks | `store.rs`: `MemoryStore` locks use `.lock().unwrap()`; async methods stay async | compile OK |
| `map_row` return type mismatch | `store.rs`: `fn map_row(...) -> rusqlite::Result<DeviceRecord>` | compile OK |
| Stray duplicate definitions / empty `map_row` | removed duplicate `DeviceRecord` and empty block | compile OK |
| Duplicate device registration returns 500 | `store.rs`: `INSERT OR REPLACE`; `MemoryStore::insert_device` uses `or_insert(...)` | idempotent 201/409 behavior restored |
| Store errors always return 500 | `error.rs`: `StoreError::NotFound` → `AgentError::NotFound` | 404s propagate correctly |

## P1-2 — Controller refactor: HashMap → SQLite `DeviceStore`

Kế thừa should-fix từ `docs/REVIEW-P1.md`: bổ sung state machine + transition path cho `DeviceState`.

| Thay đổi | File | Mục đích |
|---|---|---|
| Storage layer | `crates/sdwan-agent/src/store.rs` (378 LOC) + `migrations/001_init.sql` | SQLite-backed `DeviceStore` thay in-memory `HashMap`; idempotent register; 404 propagate qua `StoreError::NotFound` |
| Controller endpoints | `crates/sdwan-agent/src/controller.rs` | Thêm `GET /api/v1/devices`, `GET /api/v1/devices/:id`, `DELETE /api/v1/devices/:id`; wiring qua `DeviceStore` |
| Module wiring | `crates/sdwan-agent/src/lib.rs` | Export `store` module |
| Doc update | `crates/sdwan-agent/src/controller.rs` (header) | Bảng endpoint mới + ghi rõ storage backend |

## Verification

- `cargo check --workspace`: PASS (3 warning, không lỗi)
- `cargo test --workspace`: PASS (108 tests, 0 fail, 37 suites)
