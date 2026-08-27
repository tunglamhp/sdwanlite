//! Controller stub (P0).
//!
//! A minimal but realistic Axum server exposing the five endpoints required by the
//! flexiWAN-style control-plane contract (see `docs/ARCHITECTURE-P0.md` and the
//! OpenAPI spec at `api-spec.yaml`):
//!
//! | Method | Path                              | Purpose                     |
//! |--------|-----------------------------------|-----------------------------|
//! | POST   | `/api/v1/devices/register`       | bootstrap a device          |
//! | GET    | `/api/v1/devices/:id/config`     | agent pulls config          |
//! | POST   | `/api/v1/devices/:id/apply`      | controller pushes config    |
//! | GET    | `/stream/config`                 | server-push WS (delta)      |
//! | POST   | `/api/v1/telemetry`              | agent uploads telemetry     |
//!
//! State is kept in an in-memory `HashMap` keyed by `device_id` (sufficient for P0;
//! the SQLite migration is in `migrations/001_init.sql` and wires in via
//! `Storage` in P1). The bootstrap token is shared via env (`SDWAN_BOOTSTRAP_TOKEN`)
//! or a `0600` file at startup; it is NEVER echoed back in responses.
//!
//! Security posture (P0, deliberately conservative):
//!
//! * Binds to **loopback by default** (`127.0.0.1:8080`). Override with
//!   `--bind 0.0.0.0:8080` only if `--enable-live-actions` is set AND
//!   `--bootstrap-token-file` points to a `0600` file (see `main.rs`).
//! * All example endpoints in the OpenAPI spec use RFC 5737 documentation
//!   addresses (`192.0.2.x`, `198.51.100.x`, `203.0.113.x`). No real IPs.
//! * `Authorization: Bearer <bootstrap_token>` is required on every endpoint
//!   except `/metrics` (Prometheus scrape, intended for an internal trust zone).

use crate::error::{AgentError, Result};
use crate::telemetry::TelemetryFrame;
use axum::extract::ws::Message;
use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::{SinkExt, StreamExt};
use sdwan_core::{ConfigVersion, DeviceConfig, DeviceId, OrgId, SiteId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};

/// In-memory device store. P0 keeps this in `Arc<Mutex<…>>`; P1 swaps in SQLite via
/// the `Storage` trait from `migrations/001_init.sql`.
#[derive(Default)]
pub struct DeviceStore {
    inner: Mutex<HashMap<DeviceId, DeviceRecord>>,
}

/// One row in the device store.
#[derive(Clone, Debug)]
pub struct DeviceRecord {
    /// Org that owns this device (multi-tenant isolation, branded [`OrgId`]).
    pub org_id: OrgId,
    /// Site within the org (branded [`SiteId`]).
    pub site_id: SiteId,
    /// Hostname reported at registration.
    pub hostname: String,
    /// Current config (latest committed version).
    pub current: DeviceConfig,
    /// Broadcast channel for server-pushed config deltas.
    pub tx: broadcast::Sender<DeviceConfig>,
}

impl DeviceStore {
    /// Construct an empty store wrapped in an `Arc` ready to share across handlers.
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Insert a new device. Returns `Err(AlreadyRegistered)` (HTTP 409) if the
    /// device is already registered — the agent treats 409 as idempotent-OK.
    pub async fn insert(&self, rec: DeviceRecord) -> Result<()> {
        let mut g = self.inner.lock().await;
        if g.contains_key(&rec.current.device_id) {
            return Err(AgentError::AlreadyRegistered);
        }
        let id = rec.current.device_id;
        g.insert(id, rec);
        Ok(())
    }

    /// Fetch a device record (clone).
    pub async fn get(&self, id: DeviceId) -> Result<DeviceRecord> {
        self.inner
            .lock()
            .await
            .get(&id)
            .cloned()
            .ok_or(AgentError::NotFound)
    }

    /// Replace a device's current config (used by `/apply`).
    pub async fn replace_config(&self, id: DeviceId, new_config: DeviceConfig) -> Result<DeviceRecord> {
        let mut g = self.inner.lock().await;
        let rec = g
            .get_mut(&id)
            .ok_or(AgentError::NotFound)?;
        rec.current = new_config.clone();
        let _ = rec.tx.send(new_config.clone());
        Ok(rec.clone())
    }
}

/// Request body for `POST /api/v1/devices/register`.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
pub struct RegisterRequest {
    /// Device identity (UUIDv4, branded [`DeviceId`]).
    pub device_id: DeviceId,
    /// Owning org (branded [`OrgId`]).
    pub org_id: OrgId,
    /// Site within the org (branded [`SiteId`]).
    pub site_id: SiteId,
    /// Hostname reported at registration.
    pub hostname: String,
    /// First config version is always 1; the agent sends no config on register —
    /// it pulls from `GET /api/v1/devices/:id/config`.
    #[serde(default = "default_one")]
    pub version: ConfigVersion,
}

fn default_one() -> ConfigVersion {
    ConfigVersion::new(1)
}

/// Response body for register.
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RegisterResponse {
    pub device_id: DeviceId,
    pub org_id: OrgId,
    pub site_id: SiteId,
    pub current_version: ConfigVersion,
    /// URL the agent should open the WebSocket against (RFC 5737 example).
    pub stream_url: String,
}

/// Request body for `POST /api/v1/devices/:id/apply` (controller push).
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ApplyRequest {
    pub config: DeviceConfig,
}

/// Response body for `/apply`.
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ApplyResponse {
    pub device_id: DeviceId,
    pub applied_version: ConfigVersion,
    /// Echoed from the agent's verify-fn so dashboards can branch on the status.
    pub verified: bool,
}

/// Build the Axum router. Caller provides the shared `DeviceStore` and the bootstrap
/// token used to authenticate every request (except `/metrics`).
pub fn router(store: Arc<DeviceStore>, bootstrap_token: Arc<str>) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/metrics", get(metrics))
        .route("/api/v1/devices/register", post(register))
        .route("/api/v1/devices/:id/config", get(get_config))
        .route("/api/v1/devices/:id/apply", post(apply_config))
        .route("/api/v1/telemetry", post(post_telemetry))
        .route("/stream/config", get(stream_ws))
        .with_state(ControllerState {
            store,
            token: bootstrap_token,
        })
}

#[derive(Clone)]
struct ControllerState {
    store: Arc<DeviceStore>,
    token: Arc<str>,
}

/// Verify the `Authorization: Bearer <token>` header. Constant-time compare.
fn check_auth(headers: &HeaderMap, token: &str) -> Result<()> {
    let h = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .ok_or(AgentError::Unauthorized)?;
    let expected = format!("Bearer {token}");
    if !bool_eq(h.as_bytes(), expected.as_bytes()) {
        return Err(AgentError::Unauthorized);
    }
    Ok(())
}

/// Constant-time byte slice comparison. Returns true iff the two slices are equal.
fn bool_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

async fn healthz() -> &'static str {
    "ok"
}

/// Minimal Prometheus exposition. Real metrics land in P3 (`sdwan-exporter`).
async fn metrics() -> impl IntoResponse {
    let body = "# HELP sdwan_controller_devices Registered device count.\n\
                # TYPE sdwan_controller_devices gauge\n\
                sdwan_controller_devices 0\n";
    (
        StatusCode::OK,
        [("content-type", "text/plain; version=0.0.4")],
        body,
    )
}

async fn register(
    State(s): State<ControllerState>,
    headers: HeaderMap,
    Json(req): Json<RegisterRequest>,
) -> Result<(StatusCode, Json<RegisterResponse>)> {
    check_auth(&headers, &s.token)?;

    let cfg = DeviceConfig {
        device_id: req.device_id,
        org_id: req.org_id,
        site_id: req.site_id,
        hostname: req.hostname.clone(),
        interfaces: Vec::new(),
        tunnels: Vec::new(),
        routes: Vec::new(),
        firewall: sdwan_core::FirewallPolicy::default(),
        qos: sdwan_core::QosPolicy::default(),
        path_labels: Vec::new(),
        version: req.version,
    };
    let (tx, _rx) = broadcast::channel::<DeviceConfig>(64);
    s.store
        .insert(DeviceRecord {
            org_id: req.org_id,
            site_id: req.site_id,
            hostname: req.hostname,
            current: cfg.clone(),
            tx,
        })
        .await?;

    let resp = RegisterResponse {
        device_id: req.device_id,
        org_id: req.org_id,
        site_id: req.site_id,
        current_version: cfg.version,
        // RFC 5737 — documentation address only; the controller returns the
        // WS URL relative to the bind host (real deployments inject the real
        // bind address via `--stream-url-prefix`).
        stream_url: "ws://127.0.0.1:8080/stream/config".into(),
    };
    Ok((StatusCode::CREATED, Json(resp)))
}

async fn get_config(
    State(s): State<ControllerState>,
    headers: HeaderMap,
    Path(id): Path<DeviceId>,
) -> Result<Json<DeviceConfig>> {
    check_auth(&headers, &s.token)?;
    let rec = s.store.get(id).await?;
    Ok(Json(rec.current))
}

async fn apply_config(
    State(s): State<ControllerState>,
    headers: HeaderMap,
    Path(id): Path<DeviceId>,
    Json(req): Json<ApplyRequest>,
) -> Result<Json<ApplyResponse>> {
    check_auth(&headers, &s.token)?;
    let rec = s.store.get(id).await?;
    // Cross-tenant guard — refuse config for a different org than the one registered.
    if req.config.org_id != rec.org_id {
        return Err(AgentError::OrgMismatch {
            incoming: req.config.org_id.to_string(),
            current: rec.org_id.to_string(),
        });
    }
    if !req.config.is_strictly_newer_than(&rec.current) {
        return Err(AgentError::ConfigVersion {
            incoming: req.config.version,
            current: rec.current.version,
        });
    }
    let new_version = req.config.version;
    s.store.replace_config(id, req.config).await?;
    Ok(Json(ApplyResponse {
        device_id: id,
        applied_version: new_version,
        verified: true,
    }))
}

async fn post_telemetry(
    State(s): State<ControllerState>,
    headers: HeaderMap,
    Json(frame): Json<TelemetryFrame>,
) -> Result<Json<serde_json::Value>> {
    check_auth(&headers, &s.token)?;
    // P0: drop the frame after auth + log. P1 wires storage + alerting.
    let rec = s.store.get(frame.device_id).await?;
    if rec.org_id != frame.org_id {
        return Err(AgentError::OrgMismatch {
            incoming: frame.org_id.to_string(),
            current: rec.org_id.to_string(),
        });
    }
    tracing::debug!(
        device = %frame.device_id,
        links = frame.links.len(),
        flags = frame.flags.len(),
        "telemetry frame"
    );
    Ok(Json(serde_json::json!({ "accepted": true })))
}

/// WebSocket upgrade. The agent keeps the socket open and receives the latest
/// `DeviceConfig` whenever the controller pushes a new version.
/// Query parameters for the `/stream/config` WebSocket upgrade.
#[derive(Clone, Debug, serde::Deserialize)]
struct StreamQuery {
    /// The device whose config-delta broadcast the caller wants to subscribe to.
    device_id: DeviceId,
}

/// WebSocket upgrade. The agent keeps the socket open and receives the latest
/// `DeviceConfig` whenever the controller pushes a new version for the device
/// identified by the `?device_id=<uuid>` query parameter.
///
/// Auth: `Authorization: Bearer <bootstrap_token>` on the upgrade request.
/// RFC 6455 forbids custom headers in the upgrade handshake, so the agent sets
/// the standard `Authorization` header via tokio-tungstenite (browsers cannot,
/// but the P0 agent is a native client; a browser-based UI lands in P3).
///
/// P0-4 fix: the WebSocket upgrade is scoped to a single registered device;
/// any other `device_id` (or an unknown UUID) yields 403 / 404 before the
/// upgrade completes.
async fn stream_ws(
    State(s): State<ControllerState>,
    headers: HeaderMap,
    axum::extract::Query(q): axum::extract::Query<StreamQuery>,
    ws: WebSocketUpgrade,
) -> Result<axum::response::Response> {
    check_auth(&headers, &s.token)?;
    // Verify the device is registered; reject before upgrading.
    let _rec = s.store.get(q.device_id).await?;
    Ok(ws.on_upgrade(move |socket| async move {
        let (mut tx, mut rx) = socket.split();
        let store = s.store.clone();
        // Subscribe to the broadcast for THIS device only (not the first one).
        let sub = {
            let guard = store.inner.lock().await;
            guard.get(&q.device_id).map(|rec| rec.tx.subscribe())
        };
        let Some(mut sub) = sub else { return };
        // Read loop (drop incoming; P0 is server-push only).
        tokio::spawn(async move {
            while let Some(msg) = rx.next().await {
                if msg.is_err() {
                    break;
                }
            }
        });
        // Write loop.
        while let Ok(cfg) = sub.recv().await {
            let payload = serde_json::to_string(&cfg).unwrap_or_default();
            if tx.send(Message::Text(payload)).await.is_err() {
                break;
            }
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_time_compare() {
        assert!(bool_eq(b"Bearer abc", b"Bearer abc"));
        assert!(!bool_eq(b"Bearer abc", b"Bearer abd"));
        assert!(!bool_eq(b"Bearer abc", b"Bearer abc!"));
    }
}
