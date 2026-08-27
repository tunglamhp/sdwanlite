//! End-to-end integration test: a real agent against a real in-process
//! controller over loopback TCP (Pocock style: one true end-to-end flow).
//!
//! Exercises the agent's hand-rolled HTTP/1.1 client against the Axum
//! controller on an ephemeral loopback port — no mocks, no oneshot:
//!
//!   1. `Agent::register()`        → POST /api/v1/devices/register + GET config
//!   2. `Agent::apply_config()`    → transactional apply (local commit)
//!   3. `Agent::get_telemetry()`   → POST /api/v1/telemetry
//!
//! Loopback-only (no real IPs, per AGENTS.md); the bootstrap token is a test
//! constant and never logged.

use axum::Router;
use sdwan_agent::{controller_router, Agent, AgentConfig, DeviceStore};
use sdwan_core::{ConfigVersion, DeviceConfig, DeviceId, OrgId, SiteId, ValidatedConfig};
use std::sync::Arc;
use uuid::Uuid;

const TOKEN: &str = "full-flow-token";

/// Bind the controller to an ephemeral loopback port and spawn it. Returns the
/// base URL (RFC 5737 loopback range only) and the shared store.
async fn spawn_controller() -> (String, Arc<DeviceStore>) {
    let store = DeviceStore::new();
    let app: Router = controller_router(Arc::clone(&store), Arc::from(TOKEN));
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
async fn register_get_apply_telemetry_lifecycle() {
    let (base, store) = spawn_controller().await;
    let device_id = DeviceId::from_uuid(Uuid::new_v4());
    let org_id = OrgId::from_uuid(Uuid::new_v4());
    let site_id = SiteId::from_uuid(Uuid::new_v4());

    let cfg = AgentConfig::new(base, TOKEN, device_id, org_id, site_id, "edge-full-flow")
        .expect("agent config");
    let agent = Agent::new(cfg).expect("agent init");

    // 1. register: POST register + GET /devices/:id/config from the controller.
    let registered = agent.register().await.expect("register");
    assert_eq!(registered.device_id, device_id);
    assert_eq!(registered.org_id, org_id);
    assert_eq!(
        registered.version,
        ConfigVersion::new(1),
        "first config version is 1"
    );
    assert_eq!(
        agent.current().version,
        ConfigVersion::new(1),
        "agent serves the pulled config"
    );

    // Registration is idempotent: the controller returns 409/500-with-JSON,
    // the agent still pulls and serves the current config.
    let second = agent.register().await.expect("re-register is safe");
    assert_eq!(second.version, ConfigVersion::new(1));

    // 2. transactional apply of a strictly-newer config.
    let mut next = registered.clone();
    next.version = ConfigVersion::new(2);
    let outcome = agent
        .apply_config(ValidatedConfig::try_from(next.clone()).unwrap())
        .await;
    assert!(outcome.verified, "apply must commit");
    assert_eq!(
        outcome.new_version,
        ConfigVersion::new(2),
        "active version is the pushed revision"
    );
    assert_eq!(outcome.active_version, ConfigVersion::new(2));
    assert_eq!(
        agent.current().version,
        ConfigVersion::new(2),
        "agent serves the committed config"
    );

    // 3. telemetry push to the controller.
    agent.get_telemetry().await.expect("telemetry accepted");

    // The controller still holds its own copy — agent-local applies do not
    // push back in P0 (the WS stream is the push channel).
    let rec = store.get(device_id).await.expect("device stored");
    assert_eq!(rec.current.version, ConfigVersion::new(1));
    assert_eq!(rec.hostname, "edge-full-flow");
}
