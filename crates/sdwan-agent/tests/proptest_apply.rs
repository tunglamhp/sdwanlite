//! Property-based tests for `Agent::apply_config` semantics (Pocock style).
//!
//! `apply_config` is the transactional-apply core of the agent; its contract
//! is version arithmetic, so we fuzz it with random configs and versions:
//!
//!   * `reapply_same_version_is_refused` — re-applying the config that is
//!     already active (same version) must be refused without touching state:
//!     idempotence of the apply step.
//!   * `sequential_applies_stay_monotonic` — any sequence of strictly-newer
//!     configs commits, and the active version strictly increases every step
//!     (no rollback, no version reuse).

use proptest::prelude::*;
use sdwan_agent::{Agent, AgentConfig};
use sdwan_core::{
    ConfigVersion, DeviceConfig, DeviceId, FirewallPolicy, Interface, OrgId, SiteId,
    ValidatedConfig,
};

fn make_agent() -> Agent {
    let cfg = AgentConfig::new(
        "http://127.0.0.1:65535", // unused in this test (no HTTP calls)
        "test-bootstrap",
        DeviceId::new(),
        OrgId::new(),
        SiteId::new(),
        "edge-proptest",
    )
    .expect("agent config");
    Agent::new(cfg).expect("agent init")
}

/// Random config content. The apply path in P0 ignores content (version check
/// only), so the strategy varies identity + hostname + interface names to
/// prove the version semantics hold across arbitrary payloads.
fn content_config() -> impl Strategy<Value = DeviceConfig> {
    (
        any::<[u8; 16]>(),
        any::<[u8; 16]>(),
        any::<[u8; 16]>(),
        prop::collection::vec(prop::char::range('a', 'z'), 1..=12),
        prop::collection::vec(
            prop::collection::vec(prop::char::range('a', 'z'), 1..=8),
            0..=4,
        ),
    )
        .prop_map(
            |(device_id, org_id, site_id, hostname, iface_names)| DeviceConfig {
                device_id: DeviceId::from_uuid(uuid::Uuid::from_bytes(device_id)),
                org_id: OrgId::from_uuid(uuid::Uuid::from_bytes(org_id)),
                site_id: SiteId::from_uuid(uuid::Uuid::from_bytes(site_id)),
                hostname: hostname.into_iter().collect(),
                interfaces: iface_names
                    .into_iter()
                    .map(|n| Interface {
                        name: n.into_iter().collect(),
                        addresses: Vec::new(),
                        mtu: 0,
                        path_label: None,
                    })
                    .collect(),
                tunnels: Vec::new(),
                routes: Vec::new(),
                firewall: FirewallPolicy::default(),
                qos: sdwan_core::QosPolicy::default(),
                path_labels: Vec::new(),
                version: ConfigVersion::new(0), // callers set the version under test
            },
        )
}

proptest! {
    /// Idempotence: applying a config whose version equals the active one is
    /// refused, leaves the active config untouched, and reports the unchanged
    /// active version — for any version, including 0 and u64::MAX.
    #[test]
    fn reapply_same_version_is_refused(version in any::<u64>(), mut cfg in content_config()) {
        cfg.version = ConfigVersion::new(version);
        let agent = make_agent();
        agent.set_current_for_test(cfg.clone());

        let outcome = tokio_test::block_on(agent.apply_config(
            ValidatedConfig::try_from(cfg.clone()).unwrap(),
        ));
        prop_assert!(!outcome.verified, "duplicate version must be refused");
        prop_assert_eq!(outcome.active_version, ConfigVersion::new(version));
        prop_assert_eq!(outcome.new_version, ConfigVersion::new(version));
        prop_assert!(
            outcome.error.is_some(),
            "refusal must carry a reason"
        );
        prop_assert_eq!(
            agent.current(),
            cfg,
            "state must be untouched by a refused apply"
        );
    }

    /// Monotonicity: applying any sequence of strictly-newer configs commits
    /// every step and the active version strictly increases each time; the
    /// final active version is what the agent serves.
    #[test]
    fn sequential_applies_stay_monotonic(content in prop::collection::vec(content_config(), 3..=8)) {
        let agent = make_agent();
        let mut last_active: u64 = 0;
        for (i, mut cfg) in content.into_iter().enumerate() {
            cfg.version = ConfigVersion::new(i as u64 * 2 + 1); // 1, 3, 5, … (each strictly newer)
            let outcome = tokio_test::block_on(agent.apply_config(
                ValidatedConfig::try_from(cfg).unwrap(),
            ));
            prop_assert!(outcome.verified, "apply #{} must commit", i);
            prop_assert!(
                outcome.active_version.as_u64() > last_active,
                "active version must strictly increase: {} then {}",
                last_active,
                outcome.active_version
            );
            prop_assert_eq!(outcome.active_version, outcome.new_version);
            last_active = outcome.active_version.as_u64();
        }
        prop_assert_eq!(
            agent.current().version,
            ConfigVersion::new(last_active),
            "agent must serve the last committed version"
        );
    }
}
