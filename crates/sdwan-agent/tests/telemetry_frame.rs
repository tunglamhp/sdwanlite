//! Telemetry frame serde roundtrip + controller ingestion happy path.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use sdwan_agent::{controller_router, DeviceStore, HealthFlag, LinkSample, TelemetryFrame};
use sdwan_core::{DeviceId, OrgId, SiteId};
use std::net::SocketAddr;
use std::sync::Arc;
use tower::ServiceExt;

const TOKEN: &str = "telemetry-token";

fn router() -> (axum::Router, Arc<DeviceStore>) {
    let store = DeviceStore::new();
    let app = controller_router(Arc::clone(&store), Arc::from(TOKEN));
    (app, store)
}

#[test]
fn telemetry_frame_serde_roundtrip() {
    let f = TelemetryFrame {
        device_id: DeviceId::new(),
        org_id: OrgId::new(),
        uptime_secs: 42,
        links: vec![LinkSample {
            path_label: "MPLS-Primary".into(),
            interface: "wg0".into(),
            local_endpoint: "203.0.113.7:51820".into(),
            tx_bytes: 1234,
            rx_bytes: 5678,
            peer_endpoint: Some(
                "198.51.100.10:51820"
                    .parse::<SocketAddr>()
                    .unwrap()
                    .to_string(),
            ),
        }],
        flags: vec![HealthFlag::LinkDown {
            path_label: "LTE".into(),
        }],
    };
    let j = serde_json::to_string(&f).unwrap();
    let back: TelemetryFrame = serde_json::from_str(&j).unwrap();
    assert_eq!(back, f);
}

#[tokio::test]
async fn telemetry_accepted_after_register() {
    let (app, store) = router();

    // Register the device first (telemetry is org-scoped).
    let device_id = DeviceId::new();
    let org_id = OrgId::new();
    let site_id = SiteId::new();
    let body = serde_json::json!({
        "device_id": device_id,
        "org_id": org_id,
        "site_id": site_id,
        "hostname": "edge-tel",
        "version": 1,
    });
    let _ = app
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
    // store kept alive for the duration of this test only
    let _ = &store;

    let frame = TelemetryFrame {
        device_id,
        org_id,
        uptime_secs: 99,
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
async fn telemetry_rejects_unknown_device() {
    let (app, _store) = router();
    let frame = TelemetryFrame {
        device_id: DeviceId::new(), // never registered
        org_id: OrgId::new(),
        uptime_secs: 1,
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
    assert_eq!(r.status(), StatusCode::NOT_FOUND);
}
