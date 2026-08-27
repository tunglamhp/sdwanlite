# sdwanlite Control Plane — P0 Code Review

**Reviewer:** Matt-Pocock-style (type safety + DX) · **Date:** 2026-08-27
**Scope:** `crates/sdwan-core`, `crates/sdwan-agent` (src + tests), `docs/ARCHITECTURE-P0.md`, `api-spec.yaml`, `migrations/001_init.sql`
**Mode:** review only — no code changes.

> **⚠ Snapshot caveat.** The working tree was **actively being edited** while this
> review was taken (branded-type refactor + new property/snapshot tests were landing
> in real time; file mtimes moved during the review). Findings below describe the
> tree as of **~20:07 +0700**. Structural findings (M2–M6, S1–S10, spec drift) are
> design-level and survive the refactor; the build-status finding (M1) must be
> re-verified once the P0 team lands its current work.

---

## 0. Verdict

Strong bones: branded ID newtypes, `ConfigVersion(u64)`, `BootstrapToken` (only
`as_str` accessor), `PublicKey` + structured `ValidationError`, `ValidatedConfig`
wired into `apply_config`, `ApplyState` lifecycle, constant-time token compare,
ArcSwap in the right place, genuinely good doc comments. The type-safety direction
is correct.

But the landing is **not shippable as reviewed**:

1. **Build is red** — `cargo test` fails to compile multiple targets at snapshot.
2. **The agent's network layer is hand-rolled HTTP/1.1 that ignores HTTP status
   codes and leaks memory per request.**
3. **The WebSocket sync path cannot connect** — agent and controller implement
   different auth channels for the same contract.
4. **The production telemetry loop is a silent no-op stub.**

**Findings: 7 must-fix · 10 should-fix · 12 nits** (counts at snapshot).

---

## 1. Build status at snapshot (must re-verify after the team lands)

| Target | Status at snapshot |
|---|---|
| `cargo check -p sdwan-core` | ✅ compiles (missing_docs warnings) |
| `cargo check -p sdwan-agent --lib --bins` | ❌ `error[E0277]: ConfigVersion: tracing::Value` (agent.rs logging) |
| `cargo test -p sdwan-core` | ❌ `snapshot_config` (insta) + `proptest_config` (proptest) fail to resolve/link (`E0433: unlinked crate insta/proptest`, `E0405`, `E0425`, ~55 errors) |
| `cargo test -p sdwan-agent` | ❌ previously `ApplyRequest: Serialize` (controller_routes.rs) — **fixed at 20:06:50** (derives added); remaining targets not re-verified |

`docs/ARCHITECTURE-P0.md` §7 says "`cargo test -p sdwan-agent` # 4 tests pass" —
**false at review time.**

Likely contributors: mid-edit `Cargo.toml`/`Cargo.lock` (schemars/proptest/insta
added during review) and incremental-cache corruption on the `A:` volume
("hard linking files in the incremental compilation cache failed" warnings).

---

## 2. Must-fix

### M1 — Landing does not compile; doc claim "4 tests pass" is false
`cargo test -p sdwan-core -p sdwan-agent` is red at snapshot (see §1). The
architecture doc asserts a green suite. **Fix:** finish the refactor, re-run the
full suite, update the §7 claim, and don't merge on red. *Actionable by Tester; re-verify.*

### M2 — Hand-rolled HTTP/1.1 client ignores the HTTP status line
`crates/sdwan-agent/src/agent.rs` — `http_post_json` / `http_get_json`
(~lines 384–432): the response status code is never parsed. A `401` (bad token),
`404`, `409`, or `500` is treated as success and its body is parsed as the
expected JSON; an empty body becomes `Ok(json!({}))`. Every error is swallowed.

Example consequence: `register()` documents "idempotent: returns Ok even if
already registered" — today a `409` accidentally parses as `Ok` and the flow
continues. After a real status check this behaviour changes; the doc must be
reconciled either way (see S6/N6).

**Fix:** check the status line; map to `AgentError::{Unauthorized, NotFound,
ConfigVersion, Storage}`. Prefer `reqwest` (already in the workspace for
`crates/acme` — the leftover comment at `agent.rs:230-232` says exactly this)
over a hand-rolled parser; at minimum stop computing the body twice
(`serde_json::to_string(body)` twice, `unwrap_or_default()` → silent empty
payload on serialization failure).

### M3 — `url_path()` leaks memory on every HTTP request
`agent.rs` ~line 448: `format!("/{p}").leak() as &str // P0: OK for short-lived
requests`. This is a **daemon**: telemetry every 10 s plus register/config pulls
→ unbounded heap growth over the process lifetime. `String::leak` is never "OK
for short-lived requests" in a long-running agent. **Fix:** return `String`
(or an owned path) from `url_path`.

### M4 — WebSocket sync loop cannot connect: agent/controller auth channels disagree
- Agent (`agent.rs` `ws_connect_and_drain` ~line 312): `tokio_tungstenite::connect_async(url)` sends **no `Authorization` header**; the token is sent as the *first text frame*.
- Controller (`controller.rs` `stream_ws`): `check_auth(&headers, …)` validates the **upgrade request header** → the header-less upgrade is rejected `401` → `sync_loop` retries with backoff forever.
- `api-spec.yaml` `/stream/config` documents *"the agent sends the bearer token as the first text frame after the upgrade"*; `controller.rs`'s read loop is `tokio::spawn` that **drops every incoming frame** — so even the documented in-frame channel is never checked.

Three documents/implementations, three different auth models. **Fix:** pick one —
send `Authorization: Bearer` on the upgrade via `IntoClientRequest` (recommended,
matches `check_auth` and RFC 6455 constraints the doc comment itself cites), and
delete the dead "first text frame" exchange (or implement it on the controller
and drop the header check — but not both).

### M5 — Agent never enforces the org boundary on pushed configs
`api-spec.yaml` (info.description) and `ARCHITECTURE-P0.md` (§1.2): *"the agent
MUST refuse configs addressed to a different org than the one it registered
with."* `apply_config` (both the old `DeviceConfig` and the new `ValidatedConfig`
signature) checks only `version` staleness — there is **no `new.org_id ==
self.cfg.org_id` guard**. The P0 controller is device-scoped so it never pushes
cross-org in practice, but the documented MUST is absent and this is the
multi-tenancy boundary. **Fix:** one line in `apply_config` (or the WS handler)
before `ValidatedConfig::validate` — return `ApplyOutcome { verified: false,
error: "org mismatch" }` (or `AgentError::OrgMismatch`) on mismatch.

### M6 — Production telemetry loop is a silent no-op
`main.rs` (`run_agent`, ~lines 216–240): `agent_for_telemetry(&agent)` returns
`AgentHandle { _priv: () }` whose `get_telemetry()` **does nothing** and returns
`Ok(())`; the real `Agent::get_telemetry` is never called. The 10 s telemetry
push described in the docs never sends a frame. The struct comment admits it is
a shim "to make the borrow checker happy" — the honest fix is to make the borrow
checker *unnecessary*: `Agent`'s fields (`Arc<ArcSwap<DeviceConfig>>` + tokio
`Mutex`) are already `Sync`-capable — share `Arc<Agent>` and spawn
`agent.get_telemetry()` directly. Delete the shim and its
`#[allow(dead_code)]` impl.

### M7 — Duplicate registration returns 500; spec requires 409
`controller.rs` `register` → `DeviceStore::insert` → `Err(AgentError::Storage(
"device already registered"))` → `error.rs` maps `Storage` → **500
INTERNAL_SERVER_ERROR**. `api-spec.yaml` `/api/v1/devices/register`:
**`'409': device already registered`**. The re-register contract is broken
(status contract is part of the API). **Fix:** a `DeviceAlreadyRegistered`
variant (or map `Storage`'s "already registered" case) to `409` with code
`device_already_registered`.

---

## 3. Should-fix

### S1 — `ValidatedConfig::as_mut` breaks the invariant the docs advertise
`sdwan-core/src/lib.rs` — `ValidatedConfig` doc: *"an unvalidated config cannot
reach `apply` at compile time"*. But `as_mut(&mut self) -> &mut DeviceConfig`
lets any holder mutate the inner config into an invalid state (empty interface
name, bad key, wrong org) while still holding a `ValidatedConfig`. The
compile-time guarantee is a lie the moment `as_mut` is used. **Fix:** drop
`as_mut` (or provide mutation methods that re-validate); keep `into_inner` for
the commit step. Also: `set_current_for_test` and `register()` still bypass
validation — acceptable (test seam) but worth a doc note.

### S2 — Wire model still carries strings where branded/validated types exist
- `WireGuardTunnel::public_key: String` — `PublicKey` exists but derives no
  serde, so it cannot live on the wire. Doc says "may carry any value until the
  validator runs" — the validator is the only gate. **Fix:** implement
  `Serialize`/`Deserialize` for `PublicKey` (validating on deserialize) and use
  it in the struct; or keep `String` and accept the gap explicitly.
- `PathLabel::id: Uuid` — **unbranded** while every other id got a newtype;
  inconsistent half of the refactor.
- `Route.destination`/`next_hop`, `WireGuardTunnel::endpoint`, `Interface.addresses`
  are untyped strings on the wire; `DeviceConfig::validate` does **not** parse
  CIDR/IP/endpoint, and does **not** enforce the documented `path_label` "must
  reference an existing label" cross-field invariant (both doc comments promise
  more than `validate()` delivers).

### S3 — Missing `#[non_exhaustive]` on wire enums the roadmap extends
`TunnelConfig` ("shaped to make IPsec/SSTP a drop-in addition in P1"), plus
`PathLabelKind`, `ProbeType`, `FirewallAction`, `Role`, `HealthFlag`. Adding a
variant without `#[non_exhaustive]` is a breaking change for every downstream
exhaustive `match` and a wire-contract hazard. Cost is one wildcard arm.

### S4 — `apply_config` snapshot→verify→commit is TOCTOU under concurrency
The stale check reads `current()` (an `ArcSwap` clone) **before** any lock; two
concurrent applies can both pass `is_strictly_newer_than` against the same
snapshot and both commit (last-write-wins), so `active_version == committed
version` is not guaranteed. `docs/ARCHITECTURE-P0.md` words it "sequential
successful applies keep version strictly monotonic" — the word "sequential" is
carrying the whole invariant. P0 has a single WS-loop caller (latent), but the
state machine added in the refactor makes concurrent calls stomp each other's
`ApplyState` too. **Fix:** hold one mutex across snapshot→verify→commit, or
loop with `ArcSwap::compare_and_swap` + version re-check.

### S5 — Loopback guard is a string prefix match
`agent.rs` `AgentConfig::new`: `url.starts_with("http://127.0.0.1")` also
matches `http://127.0.0.1@evil.com:8080` (userinfo) and
`http://127.0.0.1.evil.com`. The naive `url_addr`/`url_host` splitters mostly
fail closed today, but the guard's contract ("controller_url must be loopback")
does not verify the parsed authority. **Fix:** parse with `url::Url` and check
`host()` is a loopback IP (or `localhost`).

### S6 — `ApplyResponse.verified` is hardcoded `true`
`controller.rs` `/apply` handler: `verified: true` always — the real verify runs
asynchronously in the agent over WS; the REST push never sees it. The field name
implies agent verification happened. **Fix:** rename (`accepted`) or document
"controller-accepted; agent verify is async" — a dashboard branching on
`verified` will be misled.

### S7 — `api-spec.yaml` has drifted from the code
- `Interface` schema requires `[name, addresses]`; code now requires `id`
  (branded `InterfaceId`, serde-transparent → a *required UUID string on the
  wire*). Spec examples without `id` fail deserialization.
- `WireGuardTunnel` schema lacks the now-required `id`.
- `TelemetryFrame`/`LinkSample` unchanged — OK.
The team added `schemars::JsonSchema` everywhere precisely to close this loop;
wire it into a schema-generation step/test and keep the spec in lockstep.

### S8 — `AgentError` hygiene
- `AgentConfig::new`'s loopback violation returns `AgentError::Internal(String)`
  — a CLI misconfiguration labelled "internal" gives operators the wrong
  debugging frame.
- `PublicKeyDecode(String)` and `Interface { message: String }` in
  `ValidationError` — free-form strings where structured variants are possible
  (acceptable at P0, note for P1).
- Otherwise good: `IntoResponse` maps to stable codes and never echoes the full
  error — matches the `ErrorBody` contract.

### S9 — Lint/doc enforcement gaps
- `#![warn(missing_docs)]` in sdwan-core fires on `ValidationError` variants/fields
  (seen in build output) — document or `#[allow]` with reason.
- `sdwan-agent/src/lib.rs` has `// #![warn(missing_docs)]` commented out with
  "docs on top-level public items only" — nothing enforces that; re-exports are
  partially documented. Re-enable or document.
- No `[workspace.lints]` / `#![deny(clippy::unwrap_used)]` in production crates.
  The AGENTS.md security posture argues for denying unwrap/expect and allowing
  only justified exceptions (e.g. the infallible `"127.0.0.1:8080".parse()`
  literal in `main.rs`).

### S10 — `with_bumped_version` uses `saturating_add`
`ConfigVersion::saturating_add` on the optimistic-lock counter: at `u64::MAX`
the version saturates and **all future configs are rejected silently** (stale
forever). `checked_add` + explicit error would be honest. Cosmetic in practice,
but the failure mode is silent.

---

## 4. Nits

- **N1** `main.rs`: `"127.0.0.1:8080".parse().unwrap()` — infallible literal, but
  against the no-unwrap rule; use `expect` or `const`.
- **N2** `main.rs` `run_agent`: `device_id`/`org_id`/`site_id` default to fresh
  `Uuid::new_v4()` **every run** — a restart re-registers as a *new* device.
  `AgentConfig::device_id` doc says "generated once and persisted on-device";
  help text says "(default: random)". Persist or document the restart semantics.
- **N3** `tests/register_flow.rs`: `_types_used`/`_unused` dead-code shims
  ("will be used once we wire a live-listener test") and an unused `body`
  variable — delete unused imports instead of silencing them.
- **N4** `tests/register_flow.rs` `trait DefaultConfigExt::default_with` — the
  empty-config constructor is now duplicated in three places (test trait,
  `controller.rs` `register`, `agent.rs` `Agent::new`). Add
  `DeviceConfig::empty(device_id, org_id, site_id)` in core.
- **N5** `agent.rs`: stale struct comment ("tests in tests/ use
  agent.current().store via pub fn below") and `set_current_for_test`'s doc
  comment is truncated mid-sentence ("Production code uses ;").
- **N6** `agent.rs` `register()` doc: "Idempotent: returns Ok even if already
  registered" — currently only true by accident of M2; reconcile doc+code.
- **N7** `controller.rs` `stream_ws` doc: "any other device_id … yields 403 /
  404" — code yields only 404 (`NotFound`); the 403 never happens on the WS path.
- **N8** `telemetry.rs` `empty_frame` has `#[allow(dead_code)]` while being pub +
  re-exported — either use it (it should feed the M6 telemetry fix) or remove.
- **N9** `ApplyOutcome.new_version` semantics differ between success (bumped)
  and failure (incoming) — document the distinction vs `active_version`.
- **N10** `tests/telemetry_frame.rs`: `let _ = &store;` — dead line.
- **N11** `post_telemetry` returns 404 for an unknown device; the spec lists only
  200/401/403 for `/api/v1/telemetry` — either spec-out the 404 or return 403.
- **N12** `telemetry_frame.rs`/`wg_pubkey.rs`: `short_payload` unused variable
  and a "hello" decode comment that admits the assertion is fuzzy
  ("just check we get a decode-length OR decode error") — tighten once proptest
  lands (the new `proptest_config.rs` covers this properly).

---

## 5. Checklist (as reviewed)

| Item | Status | Notes |
|---|---|---|
| Branded types | ✅ (in flight) | `DeviceId/OrgId/SiteId/TunnelId/InterfaceId` newtypes, serde-transparent; `ConfigVersion(u64)`; `BootstrapToken` w/ only-`as_str`; gap: `PathLabel::id` unbranded, `public_key` still `String` (S2) |
| `Result<T, E>` | ✅ | `sdwan_core::ValidationError` + `sdwan_agent::AgentError` (thiserror), crate `Result` alias; `anyhow` confined to the binary |
| `#[non_exhaustive]` | ❌ | none on wire enums (S3) |
| Doc comments | ✅ / ⚠ | generally excellent; `missing_docs` warnings unfixed; `missing_docs` disabled in agent lib; truncated comment (N5) |
| Errors actionable | ✅ / ⚠ | structured, stable machine codes, never echoes internals (M7 breaks the 409 contract); `Internal` used for CLI errors (S8) |
| No `unwrap()` in production | ⚠ | `main.rs` const-literal unwrap (N1); hand-rolled HTTP uses `unwrap_or_default` to mask serialization failure (M2) |
| ArcSwap placement | ✅ | `current: Arc<ArcSwap<DeviceConfig>>` is the right tool (lock-free reads for data-plane watchers); commit-only `store` is correct — but see TOCTOU (S4) |

---

## 6. Spec conformance (code vs ARCHITECTURE-P0.md + api-spec.yaml)

| Requirement | Where | Status |
|---|---|---|
| 5 REST endpoints + /metrics + /healthz | `controller.rs` | ✅ all present, routes match spec paths |
| Bearer auth on every endpoint except /metrics | `check_auth` | ✅ constant-time compare |
| Loopback default; `--enable-live-actions` gate | `main.rs` `is_loopback` | ✅ (guard itself string-based upstream — S5) |
| 409 on re-register | `register`/`insert` | ❌ **500** (M7) |
| 403 on org mismatch (apply, telemetry) | handlers | ✅ |
| 409 stale config version (apply) | `apply_config` | ✅ |
| Agent MUST refuse cross-org configs | agent apply path | ❌ missing (M5) |
| WS: token as first text frame | both halves | ❌ contradictory (M4) |
| Transactional apply invariants | `apply_config` + `transactional_apply.rs` | ⚠ sequential-only (S4); tests don't compile at snapshot |
| In-memory `DeviceStore`; migration declarative | store + `001_init.sql` | ✅ |
| `cargo test -p sdwan-agent # 4 tests pass` | doc §7 | ❌ false at snapshot (M1) |
| Wire-private keys never serialized | core types | ✅ `public_key` is the public key only |
| RFC 5737 examples only | docs, tests, comments | ✅ |
| Error body: stable code, "see server logs" | `AgentError::IntoResponse` | ✅ |

Minor migration note: `001_init.sql` adds `ON DELETE CASCADE/RESTRICT` and
`telemetry_frames`/`health_flags` beyond what §4 quotes — harmless, but the doc's
quoted schema is now stale. Also `devices.site_id … ON DELETE RESTRICT` vs
`sites.org_id … ON DELETE CASCADE` makes org deletion fail by FK ordering
(cascade to sites is restricted by devices) — latent until P1 deletes land.

---

## 7. Positives (keep)

- Branded-id refactor (transparent newtypes, zero wire cost) is the right call
  and lands cleanly.
- `BootstrapToken` making accidental secret logging a compile error.
- `ValidatedConfig` + `TryFrom` wired into `apply_config` — the apply path now
  receives only validated configs (modulo S1).
- `ApplyState` lifecycle mirrors the architecture sequence diagram.
- Constant-time `bool_eq` with tests; `Unauthorized` instead of `Http("invalid
  token")` (improved in the in-flight diff).
- `ValidationError` is structured with context (interface/tunnel/index).
- ArcSwap used correctly for hot config swap.
- The new proptest (`proptest_config.rs`) and insta snapshot
  (`snapshot_config.rs`) tests are exactly the right shape for this crate — once
  they build.

---

## 8. Suggested merge order

1. Land the branded-type refactor + test updates (build green first — M1).
2. M4 (WS auth), M2/M3 (HTTP), M5 (org guard), M6 (telemetry) — the agent's
   network layer needs one owner pass.
3. M7 (409) + S6 (verified semantics) — API contract correctness.
4. S1–S3, S5, S9 — type-safety and lint hardening.
5. Regenerate `api-spec.yaml` from the schemars output (S7) and update
   `ARCHITECTURE-P0.md` §7.
