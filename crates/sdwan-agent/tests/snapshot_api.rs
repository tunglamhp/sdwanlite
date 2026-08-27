//! Snapshot tests for the control-plane API response shapes (Pocock style:
//! insta). Pins the exact JSON the controller returns so a wire-format change
//! fails CI with a readable diff instead of silently breaking agents.
//!
//! UUID identity fields are redacted to keep snapshots deterministic across
//! runs. All example addresses are RFC 5737 documentation ranges; the
//! bootstrap token is a test constant and never logged.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use sdwan_agent::{
    controller_router, ApplyRequest, ApplyResponse, DeviceStore, LinkSample, RegisterResponse,
    TelemetryFrame,
};
use sdwan_core::{
    ConfigVersion, DeviceConfig, DeviceId, FirewallAction, FirewallPolicy, FirewallRule,
    HealthCheckConfig, Interface, OrgId, PathLabel, PathLabelKind, ProbeType, QosClass, QosPolicy,
    Route, SiteId, TunnelConfig, WireGuardTunnel,
};
use std::sync::Arc;
use tower::ServiceExt;
use uuid::Uuid;

const TOKEN: &str = "snapshot-token";

fn uuid(n: u128) -> Uuid {
    Uuid::from_u128(n)
}

fn app() -> (axum::Router, Arc<DeviceStore>) {
    let store = DeviceStore::new();
    let app = controller_router(Arc::clone(&store), Arc::from(TOKEN));
    (app, store)
}

/// Register a device with random identity; returns the device/org/site ids.
async fn register_one(app: &axum::Router) -> (DeviceId, OrgId, SiteId) {
    let device_id = DeviceId::new();
    let org_id = OrgId::new();
    let site_id = SiteId::new();
    let body = serde_json::json!({
        "device_id": device_id,
        "org_id": org_id,
        "site_id": site_id,
        "hostname": "edge-snapshot",
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
    (device_id, org_id, site_id)
}

fn full_config(device_id: DeviceId, org_id: OrgId, site_id: SiteId) -> DeviceConfig {
    DeviceConfig {
        device_id,
        org_id,
        site_id,
        hostname: "edge-snapshot".into(),
        interfaces: vec![Interface {
            name: "eth0".into(),
            addresses: vec!["192.0.2.10".parse().unwrap()],
            mtu: 1500,
            path_label: Some("MPLS-Primary".into()),
        }],
        tunnels: vec![TunnelConfig::WireGuard(WireGuardTunnel {
            interface: "wg0".into(),
            path_label: "MPLS-Primary".into(),
            health_check: HealthCheckConfig {
                interval_ms: 1000,
                probe_type: ProbeType::Icmp,
                threshold: 3,
                timeout_ms: 500,
            },
            endpoint: "203.0.113.7:51820".into(),
            allowed_ips: vec!["198.51.100.10".parse().unwrap()],
            public_key: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".into(),
        })],
        routes: vec![Route {
            destination: "10.0.0.0/8".into(),
            next_hop: "192.0.2.1".into(),
            metric: 100,
        }],
        firewall: FirewallPolicy {
            rules: vec![FirewallRule {
                action: FirewallAction::Accept,
                source: Some("192.0.2.0/24".into()),
                destination: None,
                protocol: Some("tcp".into()),
                port: Some(443),
                comment: Some("management".into()),
            }],
        },
        qos: QosPolicy {
            classes: vec![QosClass {
                name: "voip".into(),
                dscp: 46,
                bandwidth_bps: 1_000_000,
            }],
        },
        path_labels: vec![PathLabel {
            id: uuid(7),
            name: "MPLS-Primary".into(),
            kind: PathLabelKind::Mpls,
            sla: "loss<0.1% rtt<10ms".into(),
        }],
        version: ConfigVersion::new(5),
    }
}

#[tokio::test]
async fn register_response_shape() {
    let (app, _store) = app();
    let device_id = DeviceId::new();
    let org_id = OrgId::new();
    let site_id = SiteId::new();
    let body = serde_json::json!({
        "device_id": device_id,
        "org_id": org_id,
        "site_id": site_id,
        "hostname": "edge-snapshot",
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
    let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
    let parsed: RegisterResponse = serde_json::from_slice(&bytes).unwrap();
    insta::assert_json_snapshot!("register_response", parsed, {
        ".device_id" => "[uuid]",
        ".org_id" => "[uuid]",
        ".site_id" => "[uuid]",
    });
}

#[tokio::test]
async fn apply_response_shape() {
    let (app, _store) = app();
    let (device_id, org_id, site_id) = register_one(&app).await;
    let req = ApplyRequest {
        config: full_config(device_id, org_id, site_id),
    };
    let resp = app
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
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
    let parsed: ApplyResponse = serde_json::from_slice(&bytes).unwrap();
    insta::assert_json_snapshot!("apply_response", parsed, {
        ".device_id" => "[uuid]",
    });
}

#[tokio::test]
async fn get_config_full_wire_contract() {
    let (app, _store) = app();
    let (device_id, org_id, site_id) = register_one(&app).await;
    let req = ApplyRequest {
        config: full_config(device_id, org_id, site_id),
    };
    let _ = app
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

    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/devices/{device_id}/config"))
                .header("authorization", format!("Bearer {TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
    let parsed: DeviceConfig = serde_json::from_slice(&bytes).unwrap();
    insta::assert_json_snapshot!("get_config_full_wire_contract", parsed, {
        ".device_id" => "[uuid]",
        ".org_id" => "[uuid]",
        ".site_id" => "[uuid]",
    });
}

#[tokio::test]
async fn telemetry_accepted_response_shape() {
    let (app, _store) = app();
    let (device_id, org_id, _site_id) = register_one(&app).await;
    let frame = TelemetryFrame {
        device_id,
        org_id,
        uptime_secs: 12,
        links: vec![LinkSample {
            path_label: "MPLS-Primary".into(),
            interface: "wg0".into(),
            local_endpoint: "203.0.113.7:51820".into(),
            tx_bytes: 1234,
            rx_bytes: 5678,
            peer_endpoint: Some("198.51.100.10:51820".into()),
        }],
        flags: vec![sdwan_agent::HealthFlag::LinkDown {
            path_label: "LTE-Backup".into(),
        }],
    };
    let resp = app
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
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
    let parsed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    insta::assert_json_snapshot!("telemetry_accepted", parsed);
}

/// The agent→controller telemetry frame shape (what `Agent::get_telemetry`
/// uploads) is a wire contract too.
#[test]
fn telemetry_frame_wire_contract() {
    let frame = TelemetryFrame {
        device_id: DeviceId::from_uuid(uuid(1)),
        org_id: OrgId::from_uuid(uuid(2)),
        uptime_secs: 3600,
        links: vec![LinkSample {
            path_label: "MPLS-Primary".into(),
            interface: "wg0".into(),
            local_endpoint: "203.0.113.7:51820".into(),
            tx_bytes: 1_000_000,
            rx_bytes: 2_000_000,
            peer_endpoint: Some("198.51.100.10:51820".into()),
        }],
        flags: vec![sdwan_agent::HealthFlag::Degraded {
            subsystem: "bgp".into(),
        }],
    };
    insta::assert_json_snapshot!("telemetry_frame_wire_contract", frame);
}
