//! Integration test: transactional apply commit / rollback semantics.
//!
//! Mirrors the contract described in `docs/ARCHITECTURE-P0.md`:
//!   * `verify_fn` returning `Ok(())`  → new config becomes active and version+1
//!   * `verify_fn` returning `Err(_)`  → previous config stays active, version unchanged

use sdwan_agent::{Agent, AgentConfig};
use sdwan_core::{DeviceConfig, FirewallPolicy, QosPolicy};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use uuid::Uuid;

fn sample_config(version: u64) -> DeviceConfig {
    DeviceConfig {
        device_id: Uuid::new_v4(),
        org_id: Uuid::new_v4(),
        site_id: Uuid::new_v4(),
        hostname: format!("edge-{version}"),
        interfaces: Vec::new(),
        tunnels: Vec::new(),
        routes: Vec::new(),
        firewall: FirewallPolicy::default(),
        qos: QosPolicy::default(),
        path_labels: Vec::new(),
        version,
    }
}

fn make_agent() -> Agent {
    let cfg = AgentConfig::new(
        "http://127.0.0.1:65535", // unused in this test (no HTTP calls)
        "test-bootstrap",
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        "edge-test",
    )
    .expect("agent config");
    Agent::new(cfg).expect("agent init")
}

#[tokio::test]
async fn verify_ok_commits_new_config_and_bumps_version() {
    let agent = make_agent();
    let initial = sample_config(1);
    agent.set_current_for_test(initial.clone());

    let counter = Arc::new(AtomicU32::new(0));
    let counter_for_verify = counter.clone();
    agent
        .set_verify(Box::new(move |_cfg| {
            counter_for_verify.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }))
        .await;

    let incoming = sample_config(2);
    let outcome = agent.apply_config(incoming.clone()).await;

    assert!(outcome.verified);
    assert_eq!(outcome.new_version, 3);
    assert_eq!(outcome.active_version, 3);
    assert!(outcome.error.is_none());
    assert_eq!(counter.load(Ordering::SeqCst), 1, "verify called once");

    let active = agent.current();
    assert_eq!(active.version, 3);
    assert_eq!(active.hostname, incoming.hostname);
}

#[tokio::test]
async fn verify_err_rolls_back_and_does_not_bump_version() {
    let agent = make_agent();
    let initial = sample_config(5);
    let initial_hostname = initial.hostname.clone();
    agent.set_current_for_test(initial.clone());

    agent
        .set_verify(Box::new(|_cfg| Err("simulated verify failure".into())))
        .await;

    let incoming = sample_config(7);
    let incoming_hostname = incoming.hostname.clone();
    let outcome = agent.apply_config(incoming).await;

    assert!(!outcome.verified);
    assert_eq!(outcome.new_version, 7, "incoming version reported back");
    assert_eq!(
        outcome.active_version, 5,
        "active version unchanged after rollback"
    );
    assert!(outcome.error.is_some());

    let active = agent.current();
    assert_eq!(
        active.version, 5,
        "snapshot must remain live (version not bumped)"
    );
    assert_eq!(
        active.hostname, initial_hostname,
        "old hostname must still be active"
    );
    assert_ne!(
        active.hostname, incoming_hostname,
        "incoming hostname must NOT have leaked through"
    );
}

#[tokio::test]
async fn stale_version_is_refused_without_calling_verify() {
    let agent = make_agent();
    agent.set_current_for_test(sample_config(10));

    let counter = Arc::new(AtomicU32::new(0));
    let counter_for_verify = counter.clone();
    agent
        .set_verify(Box::new(move |_cfg| {
            counter_for_verify.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }))
        .await;

    // version 10 <= 10 → rejected before verify.
    let outcome = agent.apply_config(sample_config(10)).await;
    assert!(!outcome.verified);
    assert_eq!(outcome.active_version, 10);
    assert!(outcome.error.is_some());
    assert_eq!(
        counter.load(Ordering::SeqCst),
        0,
        "verify must not be called for stale version"
    );
}

#[tokio::test]
async fn sequential_verifies_keep_version_monotonic() {
    let agent = make_agent();
    agent.set_current_for_test(sample_config(1));

    let calls = Arc::new(AtomicU32::new(0));
    let calls_for_verify = calls.clone();
    agent
        .set_verify(Box::new(move |_cfg| {
            calls_for_verify.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }))
        .await;

    // incoming versions stay well above the post-bump active to keep the
    // strict-newer check honest across iterations.
    let seq = [10u64, 20, 30, 40];
    for v in seq {
        let outcome = agent.apply_config(sample_config(v)).await;
        assert!(outcome.verified, "v{v} should verify");
        assert_eq!(outcome.active_version, v + 1);
    }
    assert_eq!(calls.load(Ordering::SeqCst), 4);
    assert_eq!(agent.current().version, 41);
}
