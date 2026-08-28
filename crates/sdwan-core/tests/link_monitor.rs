//! Link Monitor (flexiWAN §9) type tests + DeviceConfig validation tests.

use sdwan_core::{
    ConfigVersion, DeviceConfig, DeviceId, FirewallAction, FirewallPolicy, FirewallRule,
    HealthCheckConfig, Interface, OrgId, PathLabel, PathLabelKind, ProbeType, QosClass, QosPolicy,
    Route, SiteId, TunnelConfig, ValidationError, WireGuardTunnel,
};
use uuid::Uuid;

fn sample_path_label(name: &str) -> PathLabel {
    PathLabel {
        id: Uuid::new_v4(),
        name: name.into(),
        kind: PathLabelKind::Mpls,
        sla: String::new(),
    }
}

fn sample_tunnel(public_key: &str) -> TunnelConfig {
    TunnelConfig::WireGuard(WireGuardTunnel {
        interface: "wg0".into(),
        path_label: "MPLS-Primary".into(),
        health_check: HealthCheckConfig::default(),
        endpoint: "203.0.113.7:51820".into(),
        allowed_ips: vec!["198.51.100.10".parse().unwrap()],
        public_key: public_key.into(),
    })
}

#[test]
fn probe_type_kebab_serde() {
    for (probe, name) in [
        (ProbeType::Icmp, "icmp"),
        (ProbeType::Http, "http"),
        (ProbeType::Dns, "dns"),
        (ProbeType::Tcp, "tcp"),
    ] {
        let j = serde_json::to_string(&probe).unwrap();
        assert_eq!(j, format!("\"{name}\""));
        let back: ProbeType = serde_json::from_str(&j).unwrap();
        assert_eq!(back, probe);
    }
}

#[test]
fn health_check_default_values() {
    let hc = HealthCheckConfig::default();
    assert_eq!(hc.interval_ms, 1000);
    assert_eq!(hc.threshold, 3);
    assert_eq!(hc.timeout_ms, 500);
    // ProbeType default is Icmp (via impl Default for ProbeType).
    assert_eq!(hc.probe_type, ProbeType::Icmp);
}

#[test]
fn validate_accepts_well_formed_config() {
    let mut c = DeviceConfig {
        device_id: DeviceId::new(),
        org_id: OrgId::new(),
        site_id: SiteId::new(),
        hostname: "edge-ok".into(),
        interfaces: vec![Interface {
            name: "eth0".into(),
            addresses: vec!["203.0.113.10".parse().unwrap()],
            mtu: 0,
            path_label: None,
        }],
        tunnels: vec![sample_tunnel(
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
        )],
        routes: vec![Route {
            destination: "0.0.0.0/0".into(),
            next_hop: "203.0.113.1".into(),
            metric: 100,
        }],
        firewall: FirewallPolicy::default(),
        qos: QosPolicy::default(),
        path_labels: vec![sample_path_label("MPLS-Primary")],
        version: ConfigVersion::new(1),
    };
    // Path label referenced by the tunnel must exist in path_labels.
    c.path_labels
        .push(sample_path_label(&c.tunnels[0].path_label()));
    c.validate().expect("valid config should pass");
}

#[test]
fn validate_rejects_bad_wg_public_key() {
    let mut c = DeviceConfig {
        device_id: DeviceId::new(),
        org_id: OrgId::new(),
        site_id: SiteId::new(),
        hostname: "edge-bad".into(),
        interfaces: vec![Interface {
            name: "eth0".into(),
            addresses: vec!["203.0.113.11".parse().unwrap()],
            mtu: 0,
            path_label: None,
        }],
        tunnels: vec![sample_tunnel("not-a-valid-key")],
        routes: vec![],
        firewall: FirewallPolicy::default(),
        qos: QosPolicy::default(),
        path_labels: vec![sample_path_label("MPLS-Primary")],
        version: ConfigVersion::new(1),
    };
    let label = c.tunnels[0].path_label();
    c.path_labels.push(sample_path_label(&label));
    let err = c.validate().unwrap_err();
    match err {
        ValidationError::Tunnel { interface, source } => {
            assert_eq!(interface, "wg0");
            assert!(matches!(*source, ValidationError::PublicKeyLength { .. }));
        }
        other => panic!("expected Tunnel error, got {other:?}"),
    }
}

#[test]
fn validate_rejects_empty_interface_name() {
    let mut c = DeviceConfig {
        device_id: DeviceId::new(),
        org_id: OrgId::new(),
        site_id: SiteId::new(),
        hostname: "edge-bad".into(),
        interfaces: vec![Interface {
            name: String::new(),
            addresses: vec!["203.0.113.12".parse().unwrap()],
            mtu: 0,
            path_label: None,
        }],
        tunnels: vec![sample_tunnel(
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
        )],
        routes: vec![],
        firewall: FirewallPolicy::default(),
        qos: QosPolicy::default(),
        path_labels: vec![sample_path_label("MPLS-Primary")],
        version: ConfigVersion::new(1),
    };
    c.path_labels
        .push(sample_path_label(&c.tunnels[0].path_label()));
    assert!(matches!(
        c.validate().unwrap_err(),
        ValidationError::Interface { .. }
    ));
}

#[test]
fn validate_rejects_firewall_port_zero() {
    let mut c = DeviceConfig {
        device_id: DeviceId::new(),
        org_id: OrgId::new(),
        site_id: SiteId::new(),
        hostname: "edge-bad".into(),
        interfaces: vec![],
        tunnels: vec![],
        routes: vec![],
        firewall: FirewallPolicy {
            rules: vec![FirewallRule {
                action: FirewallAction::Drop,
                source: None,
                destination: None,
                protocol: Some("tcp".into()),
                port: Some(0),
                comment: None,
            }],
        },
        qos: QosPolicy::default(),
        path_labels: vec![],
        version: ConfigVersion::new(1),
    };
    assert!(matches!(
        c.validate().unwrap_err(),
        ValidationError::FirewallRule { index: 0, .. }
    ));
}

#[test]
fn validate_rejects_qos_dscp_out_of_range() {
    let mut c = DeviceConfig {
        device_id: DeviceId::new(),
        org_id: OrgId::new(),
        site_id: SiteId::new(),
        hostname: "edge-bad".into(),
        interfaces: vec![],
        tunnels: vec![],
        routes: vec![],
        firewall: FirewallPolicy::default(),
        qos: QosPolicy {
            classes: vec![QosClass {
                name: "voip".into(),
                dscp: 64, // invalid (> 63)
                bandwidth_bps: 0,
            }],
        },
        path_labels: vec![],
        version: ConfigVersion::new(1),
    };
    assert!(matches!(
        c.validate().unwrap_err(),
        ValidationError::QosClass { index: 0, .. }
    ));
}

// Convenience: pull the path_label string out of a tunnel variant.
trait PathLabelOf {
    fn path_label(&self) -> String;
}
impl PathLabelOf for TunnelConfig {
    fn path_label(&self) -> String {
        match self {
            TunnelConfig::WireGuard(w) => w.path_label.clone(),
            // TunnelConfig is #[non_exhaustive]; P0 only defines WireGuard.
            _ => unreachable!("only WireGuard exists in P0"),
        }
    }
}
