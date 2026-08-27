//! End-to-end register flow against the in-process controller.
//!
//! Spins the controller up on `127.0.0.1:0` (OS picks a free port), registers a
//! device, then pulls its config back through `GET /api/v1/devices/:id/config`.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use sdwan_core::{ConfigVersion, DeviceId, OrgId, SiteId};
use sdwan_agent::{
    controller_router, ApplyRequest, DeviceStore, RegisterRequest, RegisterResponse,
};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tower::ServiceExt;
use uuid::Uuid;

const TOKEN: &str = "register-flow-token";

fn router(store: Arc<DeviceStore>) -> axum::Router {
    controller_router(store, Arc::from(TOKEN))
}

#[tokio::test]
async fn register_then_get_config() {
    let store = DeviceStore::new();
    let app = router(Arc::clone(&store));

    let device_id = DeviceId::new();
    let org_id = OrgId::new();
    let site_id = SiteId::new();

    let body = serde_json::json!({
        "device_id": device_id,
        "org_id": org_id,
        "site_id": site_id,
        "hostname": "edge-flow",
        "version": 1,
    });
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/devices/register")
                .header("authorization", format!("Bearer {TOKEN}"))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
    let parsed: RegisterResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(parsed.device_id, device_id);
    assert_eq!(parsed.current_version, ConfigVersion::new(1));

    // Pull config back.
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/api/v1/devices/{device_id}/config"))
                .header("authorization", format!("Bearer {TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
    let cfg: sdwan_core::DeviceConfig = serde_json::from_slice(&body).unwrap();
    assert_eq!(cfg.device_id, device_id);
    assert_eq!(cfg.version, ConfigVersion::new(1));
}

#[tokio::test]
async fn register_rejects_invalid_token() {
    let store = DeviceStore::new();
    let app = router(store);
    let body = serde_json::json!({
        "device_id": Uuid::new_v4(),
        "org_id": Uuid::new_v4(),
        "site_id": Uuid::new_v4(),
        "hostname": "edge-bad-token",
        "version": 1,
    });
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/devices/register")
                .header("authorization", "Bearer wrong-token")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn register_rejects_missing_token() {
    let store = DeviceStore::new();
    let app = router(store);
    let body = serde_json::json!({});
    let valid_body = serde_json::json!({
        "device_id": Uuid::new_v4(),
        "org_id": Uuid::new_v4(),
        "site_id": Uuid::new_v4(),
        "hostname": "edge-no-token",
        "version": 1,
    });
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/devices/register")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&valid_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn get_config_returns_404_for_unknown_device() {
    let store = DeviceStore::new();
    let app = router(store);
    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/api/v1/devices/{}/config", Uuid::new_v4()))
                .header("authorization", format!("Bearer {TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// Suppress unused-import warning on RegisterRequest (re-export smoke).
#[allow(dead_code)]
fn _types_used() {
    let _ = ApplyRequest {
        config: sdwan_core::DeviceConfig::default_with(
            DeviceId::new(),
            OrgId::new(),
            SiteId::new(),
        ),
    };
    let _: RegisterRequest = serde_json::from_str("{}").unwrap_or(RegisterRequest {
        device_id: DeviceId::new(),
        org_id: OrgId::new(),
        site_id: SiteId::new(),
        hostname: String::new(),
        version: ConfigVersion::new(1),
    });
}

// Silence unused Duration / TcpListener imports for now (will be used once
// we wire a live-listener test).
#[allow(dead_code)]
fn _unused() {
    let _: Duration = Duration::from_secs(0);
    let _: fn() -> _ = || async { TcpListener::bind("127.0.0.1:0").await };
}

trait DefaultConfigExt {
    fn default_with(
        device_id: DeviceId,
        org_id: OrgId,
        site_id: SiteId,
    ) -> sdwan_core::DeviceConfig;
}
impl DefaultConfigExt for sdwan_core::DeviceConfig {
    fn default_with(device_id: DeviceId, org_id: OrgId, site_id: SiteId) -> Self {
        Self {
            device_id,
            org_id,
            site_id,
            hostname: String::new(),
            interfaces: Vec::new(),
            tunnels: Vec::new(),
            routes: Vec::new(),
            firewall: sdwan_core::FirewallPolicy::default(),
            qos: sdwan_core::QosPolicy::default(),
            path_labels: Vec::new(),
            version: ConfigVersion::new(1),
        }
    }
}
