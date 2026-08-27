# SECURITY-REPORT-P0 — sdwan-core / sdwan-agent

**Auditor:** Matt-Pocock-Style Security Auditor (type-safe + zero-trust lens)
**Date:** 2026-08-27
**Scope:** `crates/sdwan-core`, `crates/sdwan-agent` (P0 control-plane), plus AGENTS.md compliance checks across the workspace.
**Snapshot:** Working tree at ~20:05 (uncommitted P0 changes from P0-Pocock-Architect; `HEAD = 7ec0b0b`). The tree was changing during the audit; findings reference the last-read state. Re-run after the P0 change set settles.
**Constraint honored:** No real IPs in this document; all examples use RFC 5737 (`192.0.2.x`, `198.51.100.x`, `203.0.113.x`) and loopback.

---

## Executive summary

**12 findings: 3 High, 3 Medium, 3 Low, 3 Info.**

The P0 control plane is fundamentally sound: loopback-default binding, constant-time token compare, no secrets in logs, typed route parameters, validated public keys, and — new in this pass — branded ID newtypes (`DeviceId`/`OrgId`/`SiteId`) and a `ValidatedConfig` compile-time gate. That is the right shape.

Three High findings all sit on the same seam: **the agent trusts its controller too much, and the loopback guard that is supposed to confine that trust is a string-prefix check that is trivially bypassable.** Fix those three first (F-01 → F-03); the rest are hardening.

| Severity | Count | IDs |
|---|---|---|
| High | 3 | F-01, F-02, F-03 |
| Medium | 3 | F-04, F-05, F-06 |
| Low | 3 | F-07, F-08, F-09 |
| Info | 3 | F-10, F-11, F-12 |

**Top 3 fix priorities (in order):**
1. **F-01** — parse the controller URL properly; require a real loopback host. Today `http://127.0.0.1.evil.com` passes the guard and the bootstrap token is sent to whoever that host is.
2. **F-02** — enforce `org_id`/`device_id` identity on the agent's apply path. The docs promise "agents MUST refuse configs addressed to a different org_id"; the code checks only version staleness.
3. **F-03** — wire the WebSocket auth end-to-end. The agent cannot authenticate the WS upgrade as written, so the P0 config-push channel cannot connect at all.

---

## 1. Checklist (AGENTS.md)

| Item | Verdict | Evidence |
|---|---|---|
| argv-list exec | PASS | `main.rs` parses `std::env::args()` manually; no shell. P0 crates spawn no subprocesses (`Command` grep: none in `sdwan-core`/`sdwan-agent`). |
| Atomic write | N/A in P0 | P0 crates write no files (token is read-only). Reference pattern exists in `crates/app/src/server.rs::save_path_policy` (tmp + `0o600` + `sync_all` + `rename`). |
| 0600 perms | PASS (read side) | `main.rs::read_token` rejects a token file whose mode != `0600` (unix). Cross-cutting: `crates/acme/src/renew.rs` writes the private key via `tokio::fs::write` with no mode — key file can land `0644` (data plane, fix in P1). |
| Loopback default | PARTIAL | Controller binds `127.0.0.1:8080` by default; non-loopback requires `--enable-live-actions`. Agent's URL guard is bypassable → **F-01**. |
| Secrets not logged | PARTIAL | Token never appears in logs/errors; but `BootstrapToken`/`AgentConfig` derive `Debug` → **F-06**. |
| mTLS | N/A in P0 | Single shared bearer token over loopback HTTP. mTLS/JWT are P1 work for non-loopback binds; not applicable this pass. |
| JWT | N/A in P0 | See above; `Role` enum is data only (P1 NB-API authorization). |
| Path traversal | PASS | Routes use typed extractors: `Path<DeviceId>` — non-UUID input fails parse (422); no filesystem access on any P0 route. |
| SQL injection | N/A in P0 | No SQL executed at runtime (in-memory `HashMap`); `migrations/001_init.sql` is static DDL. P1 MUST use rusqlite prepared statements / parameter binding — the schema's TEXT ids are the only interpolation surface. |
| DoS (rate limit) | FAIL | No rate limiting; unbounded store growth → **F-04**. |
| Multi-tenant isolation (org_id) | PARTIAL | Controller enforces org on `/apply` and telemetry. Agent-side identity gate missing → **F-02**. |

---

## 2. Findings

### F-01 — HIGH — Agent loopback guard is a string-prefix check → bootstrap-token exfiltration
**Where:** `crates/sdwan-agent/src/agent.rs` — `AgentConfig::new` (`url.starts_with("http://127.0.0.1") || … "http://localhost" || … "http://[::1]"`), used by `url_addr`/`url_host` and the raw HTTP helpers.
**Problem:** The guard is a prefix match, not a parsed-host check:
- `http://127.0.0.1.evil.com:8080` — starts with `http://127.0.0.1` → **passes**.
- `http://localhost.evil.com` — passes.
The raw HTTP helpers then `TcpStream::connect(url_addr(url))` and send `Authorization: Bearer <bootstrap_token>` to whatever the authority resolves to. A tampered `--controller` value (launcher config, env, operator error, supply-chain compromise) exfiltrates the shared control-plane token to an attacker host. The token grants full register/apply/WS control (and will gate the P1 data plane).
**Also:** the error message promises `--enable-live-actions` as the escape hatch, but that flag is not wired anywhere in agent mode — the message is wrong, and non-loopback controllers are unreachable by design (defense-in-depth by accident, but confusing).
**Fix:**
```rust
// parse with url::Url; host must be a loopback IpAddr (127.0.0.0/8, ::1) or the literal "localhost".
// Never DNS-resolve for the loopback decision.
```
E.g. `Url::parse` → `host_str()` → reject if it is not `Some("localhost")` or an `IpAddr` where `ip.is_loopback()`. Delete the `starts_with` set.

### F-02 — HIGH — Agent accepts configs for the wrong org/device on the wire
**Where:** `agent.rs::apply_config` (version-staleness gate only), `agent.rs::register` (stores the pulled config as-is). `sdwan-core` docs: "agents MUST refuse configs addressed to a different `org_id` than the one they registered with."
**Problem:** `ValidatedConfig` guarantees structural validity, not tenant identity — serde-transparent `OrgId`/`DeviceId` accept any UUID from the wire. `apply_config` checks only `new.version > snapshot.version`. A malicious or compromised controller (reachable via F-01, or a legit controller that was tampered) can push a newer-version config carrying a different `org_id`/`device_id`/`site_id`; the agent commits it, and P1's `verify_fn` seam will apply it to the data plane.
**Fix:** In `apply_config`, before the version gate, require:
```rust
new.as_ref().org_id == self.cfg.org_id
    && new.as_ref().device_id == self.cfg.device_id
    && new.as_ref().site_id == self.cfg.site_id
```
refuse with `AgentError::OrgMismatch`. Do the same in `register()` after the config pull, and add the identity check to `DeviceConfig::validate` so `ValidatedConfig` carries it at the type level.

### F-03 — HIGH — WebSocket auth is not wired end-to-end; the P0 sync path cannot connect
**Where:** `agent.rs::ws_connect_and_drain`, `controller.rs::stream_ws`.
**Problem:** The agent calls `tokio_tungstenite::connect_async(url)` with a bare `&str` request — **no `Authorization` header on the HTTP upgrade**. The controller runs `check_auth(&headers)` before upgrading → 401 → the agent's `sync_loop` retries forever with backoff. The agent's "inject the bearer token as the first text frame" hack is dead code: the server's read loop drops incoming messages, and the header check has already failed. Net effect: the secure config-push channel does not exist in P0 (functional break + security-control gap). The controller doc comment already admits the header-only check needs `Sec-WebSocket-Protocol` in P1.
**Fix:** Build the upgrade request via `IntoClientRequest` and add the `Authorization` header (or `Sec-WebSocket-Protocol: Bearer <token>` per the P0-4 note); delete the first-frame hack; add a route test asserting the upgrade carries the token. Document that with the single shared token, any token holder can subscribe to any registered device's stream — per-device tokens land in P1.

### F-04 — MEDIUM — No rate limiting; unbounded DeviceStore growth → DoS
**Where:** `controller.rs` — all routes; `DeviceStore::insert`.
**Problem:** `register` accepts any `device_id` (client-generated UUID) into the `HashMap` with no cap; no throttling on register/apply/telemetry/WS. A local process (the operative threat model for a loopback bind) can exhaust memory (store growth) and CPU. Axum's default 2 MiB body limit bounds payload size but not request rate.
**Fix:** Cap the store (max devices / LRU eviction), add a rate-limit layer (`tower::limit` or `Governor`) at minimum on `register` and `apply`, and move to per-device tokens in P1.

### F-05 — MEDIUM — Agent HTTP layer: per-request memory leak, no status validation, naive framing
**Where:** `agent.rs` — `url_path` (`.leak()`), `http_post_json`, `http_get_json`.
**Problem:**
- `url_path` `.leak()`s a `String` per request. Telemetry runs every 10 s per agent → unbounded resident growth; a fleet multiplies it (slow DoS).
- Response status is never checked: a 401/500 body is parsed as JSON silently; auth failures are invisible in agent logs.
- The body is split on the literal `\r\n\r\n` — chunked transfer encoding or proxy responses misparse (the 10-s telemetry loop then fails forever with a confusing `Http` error).
**Fix:** return an owned `String`; parse the status line and treat non-2xx as `AgentError::Http` (log status); cap the read buffer; in P1 replace the hand-rolled client with `reqwest` (already in the workspace via acme) or `hyper`.

### F-06 — MEDIUM — `BootstrapToken`/`AgentConfig` derive `Debug` → the secret is printable
**Where:** `sdwan-core/src/lib.rs` — `BootstrapToken` derive list.
**Problem:** The newtype's stated purpose is that accidental logging of the token is a compile error. That holds for `Display` (absent), but `Debug` is derived and prints the plaintext — `{:?}` on `AgentConfig` (also `Debug`) leaks the token into any log. Nothing currently logs it (verified), so this is defense-in-depth — but the guarantee the type claims is false.
**Fix:** manual `Debug` printing `BootstrapToken("***")`; keep `Display` absent; consider wrapping the inner `String` in `zeroize::Zeroizing<String>` so the secret is wiped on drop (Pocock-style secret handling).

### F-07 — LOW — `--bootstrap-token` on argv leaks the secret to local `ps`; non-loopback bind does not force the 0600-file path
**Where:** `main.rs` — `read_token`, `run_controller`.
**Problem:** AGENTS.md: "no secrets in argv". Both modes accept `--bootstrap-token <value>` with no runtime warning, and `run_controller` permits `--enable-live-actions` + argv token → token visible in the process list while listening on `0.0.0.0`, contradicting the controller.rs doc ("only if … `--bootstrap-token-file` points to a 0600 file").
**Fix:** `tracing::warn!` when the argv path is used; refuse non-loopback binds unless the token came from `--bootstrap-token-file`.

### F-08 — LOW — `DeviceConfig::validate` gaps become P1 data-plane injection preconditions
**Where:** `sdwan-core/src/lib.rs` — `DeviceConfig::validate`; `Route`, `WireGuardTunnel`, `Interface`.
**Problem:** `Route.destination`/`next_hop`, `WireGuardTunnel.endpoint`, `Interface.name`, `hostname`, and `path_label` references are unvalidated free-form strings. P1 renders these into iproute2/nft/wg configs (the data plane already shells out: `mesh.rs` runs `wg`, `app` renders nftables). Malformed or hostile values (`;`, spaces, control chars in an interface name or route) become command/config injection at P1.
**Fix now, while the type system is here:** newtypes `RouteDestination(IpNet)` / `Endpoint(SocketAddr)`; interface-name charset validation (alnum + `_`/`-`/`.`, no `/`, whitespace, or `;`); CIDR-parse `allowed_ips`; cross-check `path_label` against declared labels; validate `hostname`.

### F-09 — LOW — `/metrics` and `/healthz` unauthenticated (documented)
**Where:** `controller.rs` — routes.
**Problem:** Acceptable on loopback (documented as an internal trust zone). If an operator binds `0.0.0.0` with `--enable-live-actions`, device count and health become world-readable.
**Fix:** gate `/metrics` behind the token once non-loopback + TLS land; say so in `--help`.

### F-10 — INFO — No audit trail / capability newtypes (design debt vs. brief)
**Where:** `controller.rs` — `DeviceStore::insert` / `replace_config`.
**Problem:** State changes (register, apply) are not recorded beyond `tracing::debug!`; there is no `AuditLog { actor, action, before, after, timestamp }` and no capability newtypes (`AdminCapability` vs `ReadOnlyCapability`). With a single shared token, "actor" is unknowable in P0 — but the record should exist so P1 per-device tokens fill it.
**Fix:** write an `AuditLog` row on every `DeviceStore` mutation; introduce capability newtypes for the P1 NB-API.

### F-11 — INFO — Build is red; security verification blocked
**Where:** workspace-wide.
**Problem (as of the audit snapshot):**
- `cargo check -p sdwan-agent` fails: `ConfigVersion` lacks `tracing::Value` — `agent.rs` sync_loop logs `active_version = outcome.active_version` without `%`.
- `cargo clippy --all-targets` additionally fails: `ApplyRequest` derives only `Deserialize`, but `tests/controller_routes.rs` calls `serde_json::to_vec(&req)`.
- `cargo clippy -p sdwan-core -- -D warnings` fails on 22 `missing_docs` (ValidationError variants/fields — `#![warn(missing_docs)]`).
Until green: no tests run, no lint gate.
**Fix:** `%active_version` (or impl `tracing::Value`); add `Serialize` to `ApplyRequest`; document the `ValidationError` variants (or `#[expect(missing_docs)]` on the enum).

### F-12 — INFO — Supply-chain tooling results
**Where:** `cargo audit`, `cargo deny check` (no `deny.toml`/`audit.toml` committed).
**Results:**
- `cargo audit` — **0 known CVEs** across 314 dependencies. 1 warning: `rustls-pemfile 2.2.0` **unmaintained** (RUSTSEC-2025-0134, archived 2025-08), pulled via `sdwanlite-lb` (data plane) — **not in the P0 crate graph**.
- `cargo deny check` — advisories fail on the same advisory; **licenses fail** under the default policy because dual-license expressions (`MIT OR Apache-2.0`) are not in the default allowlist; bans/sources ok.
**Fix:** commit a `deny.toml` with an explicit license allowlist (MIT, Apache-2.0, BSD-3-Clause, ISC, Unicode-3.0, …); migrate the data plane off `rustls-pemfile` to `rustls-pki-types`; add `cargo audit` + `cargo deny` to CI (`.github/workflows`).

---

## 3. Positive controls (verified in this pass)

- **argv-list only** — `std::env::args()` iterator; no shell; P0 crates spawn no subprocesses.
- **Token file mode enforced** — `read_token` refuses non-`0600` token files (unix).
- **Loopback default bind; flag-gated non-loopback** — `run_controller`.
- **Token never logged**; error responses return a fixed `code` + `"see server logs"` (no internals, no UUIDs, no tokens); constant-time compare (`bool_eq`).
- **Type-level security** — branded `DeviceId`/`OrgId`/`SiteId`/`TunnelId`/`InterfaceId`, `ConfigVersion` newtype, `BootstrapToken` (no `Display`), `ValidatedConfig` as the sole constructor on the apply path. Serde-transparent wire formats preserved.
- **Input validation** — X25519 public key: length + charset + decode-to-32-bytes; interface names non-empty; addresses non-unspecified; firewall port > 0; DSCP ≤ 63.
- **RFC 5737 hygiene** — grep sweep of `sdwan-core`, `sdwan-agent`, and root configs (`sdwanlite.toml`, `pool-overrides.json`, `path-policy.json`, compose/Caddyfile/Dockerfile): no real public IPs found.
- **Path traversal / SQLi N/A** — typed `Path<DeviceId>` extractors; no runtime SQL in P0.

---

## 4. Fix priority

| # | Finding | Why first |
|---|---|---|
| 1 | F-01 loopback guard bypass → token exfil | Direct secret disclosure; trivial fix; unlocks the rest |
| 2 | F-02 agent-side org/device identity gate | Core multi-tenant promise of the crate is unenforced on the agent |
| 3 | F-03 WS auth wiring | The config-push channel (the P0 security boundary) does not connect |
| 4 | F-05 memory leak + status handling | Slow DoS; silent auth failure in agent |
| 5 | F-04 rate limit + store cap | Local DoS |
| 6 | F-06 Debug leak of `BootstrapToken` | One-line fix; closes the newtype's false guarantee |

Then F-07 → F-09 → F-08 (P1 precondition) → F-10 → F-11 → F-12.
