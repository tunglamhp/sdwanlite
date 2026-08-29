//! Device CRUD (GET/PUT/DELETE :id, PUT :id/config) and alert feed tests.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use sdwan_agent::{controller_router, DeviceStore, RegisterResponse};
use sdwan_core::{DeviceId, OrgId, SiteId};
use std::sync::Arc;
use tower::ServiceExt;

const TOKEN: &str = "crud-test-token";

fn app() -> (Router, Arc<DeviceStore>) {
    let store = DeviceStore::new();
    let router = controller_router(Arc::clone(&store), Arc::from(TOKEN));
    (router, store)
}

fn register_body(id: DeviceId, org: OrgId, site: SiteId, hostname: &str) -> String {
    serde_json::json!({
        "device_id": id,
        "org_id": org,
        "site_id": site,
        "hostname": hostname,
    })
    .to_string()
}

async fn register(router: &Router, id: DeviceId, org: OrgId, site: SiteId, hostname: &str) {
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/devices/register")
        .header("authorization", format!("Bearer {TOKEN}"))
        .header("content-type", "application/json")
        .body(Body::from(register_body(id, org, site, hostname)))
        .unwrap();
    let res = router.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
}

fn device_id() -> DeviceId {
    DeviceId::new()
}
fn org_id() -> OrgId {
    OrgId::new()
}
fn site_id() -> SiteId {
    SiteId::new()
}

#[tokio::test]
async fn get_device_returns_record_and_404_for_unknown() {
    let (router, _store) = app();
    let id = device_id();
    let org = org_id();
    let site = site_id();
    register(&router, id, org, site, "edge-a").await;

    let req = Request::builder()
        .uri(format!("/api/v1/devices/{id}"))
        .header("authorization", format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .unwrap();
    let res = router.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body: serde_json::Value =
        serde_json::from_slice(&axum::body::to_bytes(res.into_body(), 1024 * 1024).await.unwrap())
            .unwrap();
    assert_eq!(body["device_id"], serde_json::json!(id.to_string()));
    assert_eq!(body["hostname"], serde_json::json!("edge-a"));

    let unknown = device_id();
    let req = Request::builder()
        .uri(format!("/api/v1/devices/{unknown}"))
        .header("authorization", format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .unwrap();
    let res = router.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn update_device_changes_hostname() {
    let (router, _store) = app();
    let id = device_id();
    let org = org_id();
    let site = site_id();
    register(&router, id, org, site, "edge-a").await;

    let req = Request::builder()
        .method("PUT")
        .uri(format!("/api/v1/devices/{id}"))
        .header("authorization", format!("Bearer {TOKEN}"))
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::json!({ "hostname": "edge-renamed" }).to_string(),
        ))
        .unwrap();
    let res = router.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body: serde_json::Value =
        serde_json::from_slice(&axum::body::to_bytes(res.into_body(), 1024 * 1024).await.unwrap())
            .unwrap();
    assert_eq!(body["hostname"], serde_json::json!("edge-renamed"));
}

#[tokio::test]
async fn put_config_replaces_and_bumps_version() {
    let (router, _store) = app();
    let id = device_id();
    let org = org_id();
    let site = site_id();
    register(&router, id, org, site, "edge-a").await;

    let config = serde_json::json!({
        "device_id": id,
        "org_id": org,
        "site_id": site,
        "hostname": "edge-a",
        "version": 3,
        "interfaces": [],
        "tunnels": [],
        "routes": [],
        "firewall": { "rules": [] },
        "qos": { "classes": [] },
        "path_labels": []
    });
    let req = Request::builder()
        .method("PUT")
        .uri(format!("/api/v1/devices/{id}/config"))
        .header("authorization", format!("Bearer {TOKEN}"))
        .header("content-type", "application/json")
        .body(Body::from(config.to_string()))
        .unwrap();
    let res = router.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body: serde_json::Value =
        serde_json::from_slice(&axum::body::to_bytes(res.into_body(), 1024 * 1024).await.unwrap())
            .unwrap();
    assert_eq!(body["current"]["version"], serde_json::json!(3));
}

#[tokio::test]
async fn delete_device_removes_and_404s_after() {
    let (router, _store) = app();
    let id = device_id();
    let org = org_id();
    let site = site_id();
    register(&router, id, org, site, "edge-a").await;

    let req = Request::builder()
        .method("DELETE")
        .uri(format!("/api/v1/devices/{id}"))
        .header("authorization", format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .unwrap();
    let res = router.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);

    let req = Request::builder()
        .uri(format!("/api/v1/devices/{id}"))
        .header("authorization", format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .unwrap();
    let res = router.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);

    // deleting an unknown device also 404s
    let unknown = device_id();
    let req = Request::builder()
        .method("DELETE")
        .uri(format!("/api/v1/devices/{unknown}"))
        .header("authorization", format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .unwrap();
    let res = router.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn telemetry_flags_create_alerts_and_get_alerts_returns_them() {
    let (router, _store) = app();
    let id = device_id();
    let org = org_id();
    let site = site_id();
    register(&router, id, org, site, "edge-a").await;

    let frame = serde_json::json!({
        "device_id": id,
        "org_id": org,
        "uptime_secs": 60,
        "links": [],
        "flags": [
            { "kind": "link_down", "path_label": "internet" },
            { "kind": "degraded", "subsystem": "bgp" }
        ]
    });
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/telemetry")
        .header("authorization", format!("Bearer {TOKEN}"))
        .header("content-type", "application/json")
        .body(Body::from(frame.to_string()))
        .unwrap();
    let res = router.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let req = Request::builder()
        .uri("/api/v1/alerts")
        .header("authorization", format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .unwrap();
    let res = router.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body: Vec<serde_json::Value> =
        serde_json::from_slice(&axum::body::to_bytes(res.into_body(), 1024 * 1024).await.unwrap())
            .unwrap();
    let kinds: Vec<&str> = body
        .iter()
        .map(|a| a["kind"].as_str().unwrap_or(""))
        .collect();
    assert!(kinds.contains(&"link_down"), "got {kinds:?}");
    assert!(kinds.contains(&"degraded"), "got {kinds:?}");
    assert!(body[0]["title"].as_str().unwrap().contains("edge-a"));

    // same flags again → no new alerts (transition-only)
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/telemetry")
        .header("authorization", format!("Bearer {TOKEN}"))
        .header("content-type", "application/json")
        .body(Body::from(frame.to_string()))
        .unwrap();
    let res = router.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let req = Request::builder()
        .uri("/api/v1/alerts")
        .header("authorization", format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .unwrap();
    let res = router.oneshot(req).await.unwrap();
    let body: Vec<serde_json::Value> =
        serde_json::from_slice(&axum::body::to_bytes(res.into_body(), 1024 * 1024).await.unwrap())
            .unwrap();
    assert_eq!(body.len(), 2, "alerts should not duplicate: {body:?}");
}

#[tokio::test]
async fn endpoints_require_auth() {
    let (router, _store) = app();
    let id = device_id();
    for (method, uri, body, content_type) in [
        ("GET", format!("/api/v1/devices/{id}"), Body::empty(), None),
        (
            "PUT",
            format!("/api/v1/devices/{id}"),
            Body::from(serde_json::json!({ "hostname": "x" }).to_string()),
            Some("application/json"),
        ),
        ("DELETE", format!("/api/v1/devices/{id}"), Body::empty(), None),
        ("GET", "/api/v1/alerts".to_string(), Body::empty(), None),
    ] {
        let mut builder = Request::builder().method(method).uri(uri);
        if let Some(ct) = content_type {
            builder = builder.header("content-type", ct);
        }
        let req = builder.body(body).unwrap();
        let res = router.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED, "{method}");
    }
}
