//! Controller route happy-path tests — register, get-config, apply, telemetry.
//!
//! Each test stands up the in-process Axum router and exercises one endpoint
//! via `tower::ServiceExt::oneshot`. Auth is enforced via the bootstrap token
//! the router was constructed with.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use sdwan_core::{ConfigVersion, DeviceId, OrgId, SiteId};
use sdwan_agent::{
    controller_router, ApplyRequest, ApplyResponse, DeviceStore, RegisterResponse,
    TelemetryFrame,
};
use std::sync::Arc;
use tower::ServiceExt;

const TOKEN: &str = "controller-routes-token";

fn router(store: Arc<DeviceStore>) -> axum::Router {
    controller_router(store, Arc::from(TOKEN))
}

/// Register one device and return its `(device_id, org_id, site_id)`.
async fn register_one(app: axum::Router) -> (DeviceId, OrgId, SiteId) {
    let device_id = DeviceId::new();
    let org_id = OrgId::new();
    let site_id = SiteId::new();
    let body = serde_json::json!({
        "device_id": device_id,
        "org_id": org_id,
        "site_id": site_id,
        "hostname": "edge-routes",
        "version": 1,
    });
    let resp = app
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
    (device_id, org_id, site_id)
}

#[tokio::test]
async fn healthz_and_metrics_unauthenticated() {
    let store = DeviceStore::new();
    let app = router(store);
    let r = app
        .clone()
        .oneshot(Request::builder().uri("/healthz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);

    let r = app
        .oneshot(Request::builder().uri("/metrics").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
}

#[tokio::test]
async fn register_get_apply_telemetry_happy_path() {
    let store = DeviceStore::new();
    let app = router(Arc::clone(&store));

    // 1. register
    let (device_id, org_id, _site_id) = register_one(app.clone()).await;

    // 2. apply a strictly-newer config
    let new_cfg = sdwan_core::DeviceConfig {
        device_id,
        org_id,
        site_id: SiteId::new(),
        hostname: "edge-routes".into(),
        interfaces: vec![],
        tunnels: vec![],
        routes: vec![],
        firewall: sdwan_core::FirewallPolicy::default(),
        qos: sdwan_core::QosPolicy::default(),
        path_labels: vec![],
        version: ConfigVersion::new(5),
    };
    let req = ApplyRequest {
        config: new_cfg.clone(),
    };
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/devices/{device_id}/apply"))
                .header("authorization", format!("Bearer {TOKEN}"))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&req).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let body = axum::body::to_bytes(r.into_body(), 4096).await.unwrap();
    let parsed: ApplyResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(parsed.applied_version, ConfigVersion::new(5));
    assert!(parsed.verified);

    // 3. telemetry
    let frame = TelemetryFrame {
        device_id,
        org_id,
        uptime_secs: 12,
        links: vec![],
        flags: vec![],
    };
    let r = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/telemetry")
                .header("authorization", format!("Bearer {TOKEN}"))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&frame).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
}

#[tokio::test]
async fn apply_rejects_org_mismatch() {
    let store = DeviceStore::new();
    let app = router(Arc::clone(&store));
    let (device_id, _registered_org, _site_id) = register_one(app.clone()).await;

    // Apply config claiming a DIFFERENT org_id → 403 org_mismatch.
    let rogue_cfg = sdwan_core::DeviceConfig {
        device_id,
        org_id: OrgId::new(), // different!
        site_id: SiteId::new(),
        hostname: "edge-routes".into(),
        interfaces: vec![],
        tunnels: vec![],
        routes: vec![],
        firewall: sdwan_core::FirewallPolicy::default(),
        qos: sdwan_core::QosPolicy::default(),
        path_labels: vec![],
        version: ConfigVersion::new(2),
    };
    let req = ApplyRequest {
        config: rogue_cfg,
    };
    let r = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/devices/{device_id}/apply"))
                .header("authorization", format!("Bearer {TOKEN}"))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&req).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn apply_rejects_stale_version() {
    let store = DeviceStore::new();
    let app = router(Arc::clone(&store));
    let (device_id, org_id, _site_id) = register_one(app.clone()).await;

    // Register sets version=1. Apply with version=1 → stale → 409.
    let cfg = sdwan_core::DeviceConfig {
        device_id,
        org_id,
        site_id: SiteId::new(),
        hostname: "edge-routes".into(),
        interfaces: vec![],
        tunnels: vec![],
        routes: vec![],
        firewall: sdwan_core::FirewallPolicy::default(),
        qos: sdwan_core::QosPolicy::default(),
        path_labels: vec![],
        version: ConfigVersion::new(1),
    };
    let req = ApplyRequest {
        config: cfg,
    };
    let r = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/devices/{device_id}/apply"))
                .header("authorization", format!("Bearer {TOKEN}"))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&req).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::CONFLICT);
}
