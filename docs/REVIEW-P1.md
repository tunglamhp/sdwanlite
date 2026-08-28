# sdwanlite P1 Code Review — Uncommitted Diff

**Reviewer:** Pocock-style (typesafety / DX / zero-cost / naming)  
**Date:** 2026-08-28  
**Scope:** uncommitted `git diff` in `A:/web/sdwan/sdwanlite`  
**Mode:** review only — no code changes.

---

## Verdict

This diff is mostly formatting/cleanup. No merge blockers found. One semantic addition
needs follow-up: `DeviceState` is introduced and stored, but the current controller
paths don’t expose/transition it, so it’s dead weight until P1 actually uses it.

---

## Per-file verdict

| File | Verdict |
|---|---|
| `crates/sdwan-core/src/lib.rs` | OK — docs lint allowance + derive reflow + unused `DeviceState` addition needs next step |
| `crates/sdwan-agent/src/agent.rs` | OK — formatting only |
| `crates/sdwan-agent/src/controller.rs` | Should change — `DeviceState` added without usage/transition path |
| `crates/sdwan-agent/src/error.rs` | OK — formatting only |
| `crates/sdwan-agent/src/main.rs` | OK — formatting only |
| `crates/sdwan-agent/src/telemetry.rs` | OK — test blank-line cleanup |
| `crates/sdwan-agent/tests/controller_routes.rs` | OK — formatting/line-break cleanup |
| `crates/sdwan-agent/tests/register_flow.rs` | OK — formatting only |
| `crates/sdwan-agent/tests/telemetry_frame.rs` | OK — formatting only |
| `crates/sdwan-agent/tests/ws_sync.rs` | OK — formatting only |
| `crates/sdwan-core/tests/link_monitor.rs` | OK — formatting only |
| `crates/sdwan-core/tests/proptest_config.rs` | OK — formatting only |
| `crates/sdwan-core/tests/types_roundtrip.rs` | OK — formatting only |
| `crates/sdwan-core/tests/wg_pubkey.rs` | OK — formatting only |

---

## Must-fix

None. This diff does not block merge.

## Should-fix

### 1. `crates/sdwan-agent/src/controller.rs` — `DeviceState` added without usage path
This diff adds `DeviceState` and stores it on `DeviceRecord`, but current controller
handlers don’t expose it via API and don’t transition it on register/apply/telemetry.
That makes the field dead data and expands the wire/serde surface without value.

Recommended:
- either remove the `state` field/store update until lifecycle transitions are implemented,
- or add minimal API/spec usage so the field is observable and not just serialized noise.

## Nit

### 1. `crates/sdwan-core/src/lib.rs` — broad `missing_docs` allowance
This diff changes `#![warn(missing_docs)]` to allow `missing_docs`. That hides real
doc gaps instead of fixing them. Prefer doc comments on public items, not a crate-wide
allow.

### 2. Formatting-only diffs in tests
Multiple test files only change line breaks/blank lines. Fine, but avoid mixing pure
formatting into feature/refactor commits unless repo formatting is automated.

---

## Counts

| Category | Count |
|---|---|
| must-fix | 0 |
| should-fix | 1 |
| nit | 2 |

---

## Merge/CI risk

- No compile-blocking issues identified from this diff alone.
- No obvious spec breakage in changed paths.
- Main risk is accumulating unused control-plane state (`DeviceState`) without
  observable behavior.
