//! sdwan-agent: control-plane agent + in-process controller stub (P0).
//!
//! See `docs/ARCHITECTURE-P0.md` and `api-spec.yaml` for the full contract.
//!
//! Two binaries share this crate:
//!   * `sdwan-agent` (default) — runs as the on-device control-plane agent
//!   * `--mode controller`    — runs an in-process Axum controller (P0 stub;
//!     replaced by a real deployment in P1)
//!
//! Both binaries are loopback-only by default and require explicit opt-in
//! (`--enable-live-actions`) to bind to non-loopback. The bootstrap token is
//! read from `--bootstrap-token <value>` (testing) or
//! `--bootstrap-token-file <path-to-0600>` (production, per AGENTS.md).

#![deny(rust_2018_idioms)]
// #![warn(missing_docs)]  // P0: docs on top-level public items only

pub mod agent;
pub mod controller;
pub mod error;
pub mod telemetry;

pub use agent::{Agent, AgentConfig, ApplyOutcome, ApplyState, VerifyFn};
pub use controller::{
    router as controller_router, ApplyRequest, ApplyResponse, DeviceRecord, DeviceStore,
    RegisterRequest, RegisterResponse,
};
pub use error::{AgentError, Result};
pub use telemetry::{empty_frame, HealthFlag, LinkSample, TelemetryFrame};
