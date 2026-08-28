# sdwanlite Control Plane — P2 Code Review

**Reviewer:** Pocock-style · **Date:** 2026-08-28
**Scope:** `crates/sdwan-agent/src/store.rs`, `crates/sdwan-agent/migrations/001_init.sql`
**Mode:** review only — no code changes.

> **⚠ Diff caveat.** `store.rs` and `migrations/001_init.sql` are currently
> **untracked files on `dev`**, so this review treats them as an isolated landing
> unit rather than a framed branch diff. The helper files
> `fix_store.py`, `pocock_*.txt`, and `.pocock_store.diff` are noise for review
> purposes and were excluded.

---

## 0. Verdict

The change is a reasonable P1-style persistence layer, but it is **not review-ready
as a merged landing**:

1. **It does not compile in the current workspace as written.**
2. **`rusqlite` is used from sync code inside `tokio` despite `Mutex` wrappers,**
   which is a latent scalability/panic-domain problem.
3. **The `Storage` trait API leaks ownership concerns** and creates a likely
   future correctness bug around borrowed vs owned config rows.
4. **`DeviceRecord` is over-broad** and couples persistence representation to
   in-memory controller shape.
5. **The SQL schema no longer matches the architecture doc** and the P0 review
   already flagged FK ordering risk.

**Findings: 3 must-fix · 4 should-fix · 6 nits**

---

## 1. Build/landing state

This diff is **not a compilable unit in isolation**. `crates/sdwan-agent/src/store.rs`
is a new module and does not appear wired into `controller.rs` from the reviewed
state, while `rusqlite` is added as a dependency. Whether this lands as a hidden
replacement or a future P1 swap, the current shape should not be merged without
either wiring or a clearly labeled `WIP/disabled` path.

---

## 2. Must-fix

### M1 — `rusqlite::Connection` is used inside async code via sync wrappers

`SqliteStore` stores `std::sync::Mutex<rusqlite::Connection>` and performs
`conn.query_row(...)`, `conn.execute(...)`, etc., from handlers that are already
running inside `tokio`. This is a known footgun: blocking SQLite calls can occupy
a tokio worker thread, and under concurrency this becomes latency spikes or
runtime stalls.

This is especially wrong in `register`, `get_config`, and `apply_config` paths,
where controller throughput should remain bounded by async I/O, not by SQLite
lock durations.

**Fix:** use `tokio::task::spawn_blocking` for SQLite operations, or move to
an async driver that is actually intended for tokio.

### M2 — `Storage` trait borrows config state with unclear ownership

```rust
fn replace_config(
    &self,
    id: DeviceId,
    config: &DeviceConfig,
) -> Result<DeviceRecord>;
```

Returning `DeviceRecord` while also mutating state behind `&self` is fine in Rust,
but the broader `Storage` contract is inconsistent: `insert`/`list`/`get` return
persistence-owned records, while `replace_config` returns the previous record by
value with no documented ownership transfer. Future callers will either clone
unnecessarily or violate the intended copy-on-write behavior.

**Fix:** make the ownership model explicit. Either return `Option<DeviceRecord>`
with a clear comment, or split persistence concerns into separate traits.

### M3 — `DeviceRecord` conflates device metadata and full config payload

`DeviceRecord` carries `config_json: String` alongside metadata fields, but the
controller already keeps canonical `DeviceConfig` and `DeviceRecord` types in
`controller.rs`. The new module introduces a duplicate row shape with no
shared core type. This makes schema drift very likely.

**Fix:** either derive `DeviceRecord` from the existing controller record types,
or make it a strict DB row struct with explicit conversion methods.

---

## 3. Should-fix

### S1 — `MemoryStore` duplicates `DeviceStore` from `controller.rs`

Both files define `DeviceStore` and `DeviceRecord`. This is not just duplication;
it creates ambiguity about which type is canonical and which APIs are intended
to be unified later.

**Recommendation:** keep one canonical `DeviceStore` and make `MemoryStore` a
mode or backend, not a parallel type.

### S2 — `SqliteStore::open` ignores WAL/journal errors silently

```rust
let _ = conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;");
```

Both PRAGMAs can fail on some platforms or SQLite builds. Swallowing the error
means schema or FK enforcement can quietly not apply.

**Recommendation:** at least log when WAL/FK setup fails; ideally treat FK
enforcement as required rather than best-effort.

### S3 — `map_row` returns `DeviceRecord` with `config_json` as opaque text

There is no validation that `config_json` contains a parseable `DeviceConfig`,
and no schema versioning in the row. If the core config format evolves, old
rows will deserialize unpredictably.

**Recommendation:** store a schema/config version in the row and validate on
read, or fail fast with a structured error.

### S4 — `StoreError::from(thiserror::Error)` re-export is unnecessary

```rust
impl From<StoreError> for AgentError { ... }
```

Adding `thiserror::Error` to `StoreError` only to immediately re-export it via
`From<StoreError>` is premature. It also means every new `StoreError` variant
becomes part of the public API surface.

**Recommendation:** keep `StoreError` as a plain enum with explicit `From`
implementations until there is a real caller-visible benefit.

---

## 4. Nits

- **N1** `fix_store.py` is a repo-local text mutation script with no safety
  checks; it should not ship alongside reviewed Rust code.
- **N2** `StoreError::Storage` wraps `String`; prefer a dedicated error type
  with the SQLite message preserved.
- **N3** `SqliteStore::list` loads every row into memory; for a long-lived
  controller this should stream or paginate.
- **N4** `DeviceRecord { device_id, org_id, site_id, hostname, status,
  created_at, updated_at }` repeats controller fields with no shared struct.
- **N5** Migration adds `telemetry_frames` / `health_flags`-style schema
  elements that diverge from `docs/ARCHITECTURE-P0.md` §4 quoted schema.
- **N6** `configs` uses `version TEXT NOT NULL` while the controller and
  `ConfigVersion` model it as integer/u64 semantics elsewhere.

---

## 5. Spec conformance (new module vs existing standards/spec)

| Requirement | Where | Status |
|---|---|---|
| Persistent device store replaces in-memory `DeviceStore` in P1 | new module | ⚠ correct direction, not wired yet |
| DB file mode `0600` | `SqliteStore::open` | ✅ |
| WAL mode | migration | ⚠ PRAGMA errors ignored |
| Schema declared in `migrations/001_init.sql` | migration | ✅ |
| `insert / get / replace_config / list` contract preserved | `Storage` trait | ⚠ ownership semantics unclear |
| Wire-private keys never stored | schema | ✅ no private key column |
| RFC 5737 examples only | docs/tests/comments | ✅ |
| Multi-tenant `org_id`/`site_id` preserved | schema | ✅ present |
| Architecture doc §4 schema drift | migration | ❌ diverges from quoted schema |

---

## 6. Summary

- **Standards findings:** 3 must-fix, 4 should-fix, 6 nits
- **Worst standards issue:** sync `rusqlite` inside async handlers with
  `std::sync::Mutex` — real runtime risk under concurrency
- **Spec findings:** not applicable as a standalone untracked landing unit; the
  change is directionally correct but materially ahead of its wiring state

**Recommendation:** gate this change on a single owner pass that:
1. wires it into `controller.rs` or labels it explicitly disabled,
2. switches SQLite access to `spawn_blocking` or an async driver,
3. aligns `DeviceRecord` with the existing controller record type,
4. reconciles migration schema with `ARCHITECTURE-P0.md` §4.
