use sdwan_core::{DeviceConfig, DeviceId, Interface, OrgId, SiteId};

#[test]
fn device_config_rejects_empty_interface_name() {
    let bad = DeviceConfig {
        device_id: DeviceId::new(),
        org_id: OrgId::new(),
        site_id: SiteId::new(),
        hostname: "edge".into(),
        interfaces: vec![Interface {
            name: "".into(),
            addresses: vec![],
            mtu: 0,
            path_label: None,
        }],
        tunnels: vec![],
        routes: vec![],
        firewall: sdwan_core::FirewallPolicy::default(),
        qos: sdwan_core::QosPolicy::default(),
        path_labels: vec![],
        version: sdwan_core::ConfigVersion::new(1),
    };

    assert!(bad.validate().is_err());
}
