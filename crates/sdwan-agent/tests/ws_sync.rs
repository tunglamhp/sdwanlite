//! End-to-end WebSocket config push (Pocock style: one true end-to-end flow).
//!
//! Proves the P0 control loop works over the real transport:
//!
//!   1. `Agent::register()` pulls the controller's starting config (v1).
//!   2. `Agent::sync_loop()` connects to `/stream/config` — authenticating the
//!      upgrade with the `Authorization` header — and applies pushed deltas
//!      through the transactional `apply_config`.
//!   3. A controller `/apply` push (v2) arrives over the WS and commits on the
//!      agent at the pushed revision.
//!
//! This is the regression test for the WS-auth contract: the controller only
//! accepts the upgrade when the bearer header AND the `?device_id=` query
//! match a registered device.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use sdwan_agent::{controller_router, Agent, AgentConfig, ApplyRequest, DeviceStore};
use sdwan_core::{ConfigVersion, DeviceId, OrgId, SiteId};
use std::sync::Arc;
use std::time::Duration;
use tower::ServiceExt;
use uuid::Uuid;

const TOKEN: &str = "ws-sync-token";

/// Bind the controller to an ephemeral loopback port and spawn it.
async fn spawn_controller() -> (String, Arc<DeviceStore>) {
    let store = DeviceStore::new();
    let app = controller_router(Arc::clone(&store), Arc::from(TOKEN));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind loopback");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("controller serve");
    });
    (format!("http://127.0.0.1:{}", addr.port()), store)
}

#[tokio::test]
async fn ws_push_commits_config_end_to_end() {
    let (base, store) = spawn_controller().await;
    let device_id = DeviceId::from_uuid(Uuid::new_v4());
    let org_id = OrgId::from_uuid(Uuid::new_v4());
    let site_id = SiteId::from_uuid(Uuid::new_v4());

    // Register through the real agent so controller store and agent agree on v1.
    let cfg = AgentConfig::new(base, TOKEN, device_id, org_id, site_id, "edge-ws")
        .expect("agent config");
    let agent = Arc::new(Agent::new(cfg).expect("agent init"));
    agent.register().await.expect("register");
    assert_eq!(agent.current().version, ConfigVersion::new(1));

    // Run the sync loop in the background; it authenticates the WS upgrade with
    // the Authorization header and applies pushed deltas.
    let sync_agent = agent.clone();
    let sync_task = tokio::spawn(async move { sync_agent.sync_loop().await });

    // Give the WS connection time to establish (success path has no backoff).
    tokio::time::sleep(Duration::from_millis(400)).await;

    // Push a strictly-newer config via the controller's /apply endpoint.
    let mut pushed = agent.current();
    pushed.hostname = "edge-ws-v2".into();
    pushed.version = ConfigVersion::new(2);
    let app = controller_router(Arc::clone(&store), Arc::from(TOKEN));
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/devices/{device_id}/apply"))
                .header("authorization", format!("Bearer {TOKEN}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&ApplyRequest { config: pushed }).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // The push must arrive over the WS, pass validation + verify, and commit
    // on the agent at the pushed revision.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let current = agent.current();
        if current.version == ConfigVersion::new(2) && current.hostname == "edge-ws-v2" {
            break;
        }
        if tokio::time::Instant::now() >= deadline {
            panic!(
                "agent never applied the WS-pushed config (active version {:?})",
                agent.current().version
            );
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    sync_task.abort();
}
