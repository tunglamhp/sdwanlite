//! Serde roundtrip tests for the management-hierarchy types (flexiWAN §1).
//!
//! Confirms `Org`, `Site`, `Device`, `Role`, `PathLabel`, `TunnelConfig`,
//! `HealthCheckConfig`, and `DeviceConfig` all preserve values across
//! JSON serialization — the foundation of any wire compatibility test.

use sdwan_core::{
    ConfigVersion, Device, DeviceConfig, DeviceId, FirewallPolicy, HealthCheckConfig, Interface,
    Org, OrgId, PathLabel, PathLabelKind, ProbeType, QosPolicy, Role, Route, Site, SiteId,
    TunnelConfig, WireGuardTunnel,
};
use uuid::Uuid;

fn uuid_v4() -> Uuid {
    Uuid::new_v4()
}

#[test]
fn org_roundtrip() {
    let o = Org {
        id: OrgId::new(),
        name: "Acme Corp".into(),
        created_at: 1_700_000_000,
    };
    let j = serde_json::to_string(&o).unwrap();
    let back: Org = serde_json::from_str(&j).unwrap();
    assert_eq!(back, o);
}

#[test]
fn site_roundtrip() {
    let s = Site {
        id: SiteId::new(),
        org_id: OrgId::new(),
        name: "hq-east".into(),
        created_at: 1_700_000_001,
    };
    let j = serde_json::to_string(&s).unwrap();
    let back: Site = serde_json::from_str(&j).unwrap();
    assert_eq!(back, s);
}

#[test]
fn device_roundtrip() {
    let d = Device {
        id: DeviceId::new(),
        org_id: OrgId::new(),
        site_id: SiteId::new(),
        hostname: "edge-01.sdwanlite.example".into(),
        last_seen: 1_700_000_002,
    };
    let j = serde_json::to_string(&d).unwrap();
    let back: Device = serde_json::from_str(&j).unwrap();
    assert_eq!(back, d);
}

#[test]
fn role_serde_kebab() {
    // Wire format uses snake_case via serde(rename_all = "snake_case").
    for (role, name) in [
        (Role::Owner, "owner"),
        (Role::Admin, "admin"),
        (Role::Operator, "operator"),
        (Role::Viewer, "viewer"),
    ] {
        let j = serde_json::to_string(&role).unwrap();
        assert_eq!(j, format!("\"{name}\""));
        let back: Role = serde_json::from_str(&j).unwrap();
        assert_eq!(back, role);
    }
}

#[test]
fn path_label_and_link_monitor_roundtrip() {
    let pl = PathLabel {
        id: uuid_v4(),
        name: "MPLS-Primary".into(),
        kind: PathLabelKind::Mpls,
        sla: "loss<0.1%".into(),
    };
    let j = serde_json::to_string(&pl).unwrap();
    assert!(j.contains("\"type\":\"mpls\""), "kebab-case rename: {j}");
    let back: PathLabel = serde_json::from_str(&j).unwrap();
    assert_eq!(back, pl);

    let hc = HealthCheckConfig {
        interval_ms: 1000,
        probe_type: ProbeType::Icmp,
        threshold: 3,
        timeout_ms: 500,
    };
    let j = serde_json::to_string(&hc).unwrap();
    assert!(j.contains("\"probe_type\":\"icmp\""));
    let back: HealthCheckConfig = serde_json::from_str(&j).unwrap();
    assert_eq!(back, hc);
}

#[test]
fn device_config_full_roundtrip() {
    let device_id = DeviceId::new();
    let org_id = OrgId::new();
    let site_id = SiteId::new();
    let c = DeviceConfig {
        device_id,
        org_id,
        site_id,
        hostname: "edge-02".into(),
        interfaces: vec![Interface {
            name: "eth0".into(),
            addresses: vec!["203.0.113.7".parse().unwrap()],
            mtu: 0,
            path_label: Some("MPLS-Primary".into()),
        }],
        tunnels: vec![TunnelConfig::WireGuard(WireGuardTunnel {
            interface: "wg0".into(),
            path_label: "MPLS-Primary".into(),
            health_check: HealthCheckConfig::default(),
            endpoint: "203.0.113.7:51820".into(),
            allowed_ips: vec!["198.51.100.10".parse().unwrap()],
            public_key: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".into(),
        })],
        routes: vec![Route {
            destination: "0.0.0.0/0".into(),
            next_hop: "203.0.113.1".into(),
            metric: 100,
        }],
        firewall: FirewallPolicy::default(),
        qos: QosPolicy::default(),
        path_labels: vec![pl("MPLS-Primary")],
        version: ConfigVersion::new(7),
    };
    let j = serde_json::to_string(&c).unwrap();
    let back: DeviceConfig = serde_json::from_str(&j).unwrap();
    assert_eq!(back, c);
}

fn pl(name: &str) -> PathLabel {
    PathLabel {
        id: uuid_v4(),
        name: name.into(),
        kind: PathLabelKind::Mpls,
        sla: String::new(),
    }
}
