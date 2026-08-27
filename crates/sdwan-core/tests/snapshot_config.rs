//! Snapshot test for the `DeviceConfig` wire contract (Pocock style: insta).
//!
//! Pins the exact JSON shape the controller sends the agent: field names,
//! tagged-enum serialization, serde defaults, and key ordering. Any change to
//! the wire format shows up as a diff against the committed `.snap` file.
//!
//! All addresses are RFC 5737 documentation ranges; no real IPs.

use sdwan_core::{
    ConfigVersion, DeviceConfig, DeviceId, FirewallAction, FirewallPolicy, FirewallRule,
    HealthCheckConfig, Interface, OrgId, PathLabel, PathLabelKind, ProbeType, QosClass, QosPolicy,
    Route, SiteId, TunnelConfig, WireGuardTunnel,
};
use uuid::Uuid;

fn uuid(n: u128) -> Uuid {
    Uuid::from_u128(n)
}

/// Deterministic, fully-populated config exercising every field and every
/// serde default path on the wire.
fn sample_config() -> DeviceConfig {
    DeviceConfig {
        device_id: DeviceId::from_uuid(uuid(1)),
        org_id: OrgId::from_uuid(uuid(2)),
        site_id: SiteId::from_uuid(uuid(3)),
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
        version: ConfigVersion::new(7),
    }
}

#[test]
fn device_config_wire_contract() {
    insta::assert_json_snapshot!("device_config_wire_contract", sample_config());
}

/// The controller-issued "empty device" config (what an agent sees right
/// after registration, before the first push) is also part of the contract.
#[test]
fn fresh_device_config_wire_contract() {
    let cfg = DeviceConfig {
        device_id: DeviceId::from_uuid(uuid(1)),
        org_id: OrgId::from_uuid(uuid(2)),
        site_id: SiteId::from_uuid(uuid(3)),
        hostname: "edge-snapshot".into(),
        interfaces: Vec::new(),
        tunnels: Vec::new(),
        routes: Vec::new(),
        firewall: FirewallPolicy::default(),
        qos: QosPolicy::default(),
        path_labels: Vec::new(),
        version: ConfigVersion::new(1),
    };
    insta::assert_json_snapshot!("fresh_device_config_wire_contract", cfg);
}
