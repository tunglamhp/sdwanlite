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
//! | GET    | `/api/v1/devices`                | list registered devices     |
//! | GET    | `/stream/config`                 | server-push WS (delta)      |
//! | POST   | `/api/v1/telemetry`              | agent uploads telemetry     |
//!
//! State is kept in `DeviceStore` from `store.rs`: P0 defaults to the in-memory
//! backend; P1 swaps in SQLite via `DeviceStore::sqlite(path)`. The bootstrap
//! token is shared via env (`SDWAN_BOOTSTRAP_TOKEN`) or a `0600` file at startup;
//! it is NEVER echoed back in responses.
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
use crate::store::{DeviceRecord, DeviceStore};
use crate::telemetry::TelemetryFrame;
use axum::extract::ws::{Message, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::{SinkExt, StreamExt};
use sdwan_core::{ConfigVersion, DeviceConfig, DeviceId, OrgId, SiteId};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Request body for `POST /api/v1/devices/register`.
#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct RegisterRequest {
    pub device_id: DeviceId,
    pub org_id: OrgId,
    pub site_id: SiteId,
    pub hostname: String,
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
    pub verified: bool,
}

/// Build the Axum router. Caller provides the shared `DeviceStore` and the bootstrap
/// token used to authenticate every request (except `/metrics`).
pub fn router(store: Arc<DeviceStore>, bootstrap_token: Arc<str>) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/metrics", get(metrics))
        .route("/api/v1/devices/register", post(register))
        .route("/api/v1/devices/:id", get(get_device).put(update_device).delete(delete_device))
        .route("/api/v1/devices/:id/config", get(get_config).put(put_device_config))
        .route("/api/v1/devices/:id/apply", post(apply_config))
        .route("/api/v1/devices", get(list_devices))
        .route("/api/v1/telemetry", get(get_telemetry).post(post_telemetry))
        .route("/api/v1/alerts", get(get_alerts))
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

    let rec = DeviceRecord {
        org_id: req.org_id,
        device_id: req.device_id,
        site_id: req.site_id,
        hostname: req.hostname,
        state: sdwan_core::DeviceState::Connected,
        current: cfg.clone(),
        tx: Arc::new(tokio::sync::broadcast::channel::<DeviceConfig>(64).0),
    };

    s.store.insert(rec.clone()).await?;

    let resp = RegisterResponse {
        device_id: req.device_id,
        org_id: req.org_id,
        site_id: req.site_id,
        current_version: cfg.version,
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

async fn list_devices(
    State(s): State<ControllerState>,
    headers: HeaderMap,
) -> Result<Json<Vec<DeviceRecord>>> {
    check_auth(&headers, &s.token)?;
    let items = s.store.list().await?;
    Ok(Json(items))
}
async fn get_device(
    State(s): State<ControllerState>,
    headers: HeaderMap,
    Path(id): Path<DeviceId>,
) -> Result<Json<DeviceRecord>> {
    check_auth(&headers, &s.token)?;
    let rec = s.store.get(id).await?;
    Ok(Json(rec))
}

/// Partial metadata update; only fields present in the body change.
#[derive(Clone, Debug, Deserialize)]
struct UpdateDeviceBody {
    org_id: Option<OrgId>,
    site_id: Option<SiteId>,
    hostname: Option<String>,
}

async fn update_device(
    State(s): State<ControllerState>,
    headers: HeaderMap,
    Path(id): Path<DeviceId>,
    Json(body): Json<UpdateDeviceBody>,
) -> Result<Json<DeviceRecord>> {
    check_auth(&headers, &s.token)?;
    let rec = s.store.update_meta(id, body.org_id, body.site_id, body.hostname).await?;
    Ok(Json(rec))
}

async fn delete_device(
    State(s): State<ControllerState>,
    headers: HeaderMap,
    Path(id): Path<DeviceId>,
) -> Result<StatusCode> {
    check_auth(&headers, &s.token)?;
    s.store.get(id).await?; // 404 when unknown
    s.store.delete(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn put_device_config(
    State(s): State<ControllerState>,
    headers: HeaderMap,
    Path(id): Path<DeviceId>,
    Json(config): Json<DeviceConfig>,
) -> Result<Json<DeviceRecord>> {
    check_auth(&headers, &s.token)?;
    let rec = s.store.get(id).await?;
    if config.org_id != rec.org_id {
        return Err(AgentError::OrgMismatch {
            incoming: config.org_id.to_string(),
            current: rec.org_id.to_string(),
        });
    }
    let updated = s.store.replace_config(id, config).await?;
    Ok(Json(updated))
}

async fn get_alerts(
    State(s): State<ControllerState>,
    headers: HeaderMap,
) -> Result<Json<Vec<crate::store::AlertEvent>>> {
    check_auth(&headers, &s.token)?;
    Ok(Json(s.store.alerts().await))
}

async fn apply_config(
    State(s): State<ControllerState>,
    headers: HeaderMap,
    Path(id): Path<DeviceId>,
    Json(req): Json<ApplyRequest>,
) -> Result<Json<ApplyResponse>> {
    check_auth(&headers, &s.token)?;
    let rec = s.store.get(id).await?;
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

/// Latest telemetry frame per registered device.
async fn get_telemetry(
    State(s): State<ControllerState>,
    headers: HeaderMap,
) -> Result<Json<Vec<TelemetryFrame>>> {
    check_auth(&headers, &s.token)?;
    Ok(Json(s.store.latest_telemetry().await))
}

async fn post_telemetry(
    State(s): State<ControllerState>,
    headers: HeaderMap,
    Json(frame): Json<TelemetryFrame>,
) -> Result<Json<serde_json::Value>> {
    check_auth(&headers, &s.token)?;
    let rec = s.store.get(frame.device_id).await?;
    if rec.org_id != frame.org_id {
        return Err(AgentError::OrgMismatch {
            incoming: frame.org_id.to_string(),
            current: rec.org_id.to_string(),
        });
    }
    if frame.device_id != rec.current.device_id {
        return Err(AgentError::OrgMismatch {
            incoming: frame.device_id.to_string(),
            current: rec.current.device_id.to_string(),
        });
    }
    tracing::debug!(
        device = %frame.device_id,
        links = frame.links.len(),
        flags = frame.flags.len(),
        "telemetry frame"
    );
    s.store.insert_telemetry(&frame).await?;
    let hostname = rec.current.hostname.clone();
    let keys: Vec<String> = frame
        .flags
        .iter()
        .map(|f| match f {
            crate::telemetry::HealthFlag::LinkDown { path_label } => {
                format!("link_down:{path_label}")
            }
            crate::telemetry::HealthFlag::Degraded { subsystem } => {
                format!("degraded:{subsystem}")
            }
        })
        .collect();
    for key in s.store.new_alert_flags(frame.device_id, keys).await {
        let (kind, title) = match key.split_once(':') {
            Some(("link_down", label)) => {
                ("link_down", format!("{hostname}: link down ({label})"))
            }
            Some(("degraded", sub)) => ("degraded", format!("{hostname}: degraded ({sub})")),
            _ => continue,
        };
        s.store.push_alert(kind, title, None).await;
    }
    Ok(Json(serde_json::json!({ "accepted": true })))
}

/// Query parameters for the `/stream/config` WebSocket upgrade.
#[derive(Clone, Debug, serde::Deserialize)]
struct StreamQuery {
    device_id: DeviceId,
    /// Browser WebSockets cannot set headers; accept the token as a query
    /// fallback (constant-time compare, same secret as the header path).
    #[serde(default)]
    token: Option<String>,
}

async fn stream_ws(
    State(s): State<ControllerState>,
    headers: HeaderMap,
    axum::extract::Query(q): axum::extract::Query<StreamQuery>,
    ws: WebSocketUpgrade,
) -> Result<axum::response::Response> {
    let header_ok = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .map(|h| bool_eq(h.as_bytes(), format!("Bearer {}", s.token).as_bytes()))
        .unwrap_or(false);
    let query_ok = q
        .token
        .as_deref()
        .map(|t| bool_eq(t.as_bytes(), s.token.as_bytes()))
        .unwrap_or(false);
    if !(header_ok || query_ok) {
        return Err(AgentError::Unauthorized);
    }
    let _rec = s.store.get(q.device_id).await?;
    Ok(ws.on_upgrade(move |socket| async move {
        let (mut tx, mut rx) = socket.split();
        let store = s.store.clone();
        let mut sub = match store.subscribe(q.device_id).await {
            Ok(rx) => rx,
            Err(_) => return,
        };
        tokio::spawn(async move {
            while let Some(msg) = rx.next().await {
                if msg.is_err() {
                    break;
                }
            }
        });
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
    use crate::store::DeviceStore;
    use crate::telemetry::TelemetryFrame;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    #[tokio::test]
    async fn register_and_list_ok() {
        let store = DeviceStore::new();
        let token = Arc::from("token");
        let app = router(store, token);

        let register = Request::builder()
            .method("POST")
            .uri("/api/v1/devices/register")
            .header("Authorization", "Bearer token")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_vec(&RegisterRequest {
                    device_id: DeviceId::new(),
                    org_id: OrgId::new(),
                    site_id: SiteId::new(),
                    hostname: "h1".into(),
                    version: ConfigVersion::new(1),
                })
                .unwrap(),
            ))
            .unwrap();
        let res = app.clone().oneshot(register).await.unwrap();
        assert_eq!(res.status(), StatusCode::CREATED);

        let list = Request::builder()
            .method("GET")
            .uri("/api/v1/devices")
            .header("Authorization", "Bearer token")
            .body(Body::empty())
            .unwrap();
        let res = app.oneshot(list).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn telemetry_ok_after_register() {
        let store = DeviceStore::new();
        let token = Arc::from("token");
        let app = router(store, token);
        let device_id = DeviceId::new();
        let org_id = OrgId::new();
        let site_id = SiteId::new();

        let register = Request::builder()
            .method("POST")
            .uri("/api/v1/devices/register")
            .header("Authorization", "Bearer token")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_vec(&RegisterRequest {
                    device_id,
                    org_id,
                    site_id,
                    hostname: "h1".into(),
                    version: ConfigVersion::new(1),
                })
                .unwrap(),
            ))
            .unwrap();
        let res = app.clone().oneshot(register).await.unwrap();
        assert_eq!(res.status(), StatusCode::CREATED);

        let body = Request::builder()
            .method("POST")
            .uri("/api/v1/telemetry")
            .header("Authorization", "Bearer token")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_vec(&TelemetryFrame {
                    device_id,
                    org_id,
                    uptime_secs: 1,
                    links: vec![],
                    flags: vec![],
                })
                .unwrap(),
            ))
            .unwrap();
        let res = app.oneshot(body).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[test]
    fn constant_time_compare() {
        assert!(bool_eq(b"Bearer abc", b"Bearer abc"));
        assert!(!bool_eq(b"Bearer abc", b"Bearer abd"));
        assert!(!bool_eq(b"Bearer abc", b"Bearer abc!"));
    }
}
