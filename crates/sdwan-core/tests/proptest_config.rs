//! Property-based tests for the sdwan-core type system (Pocock style).
//!
//! Everything in this crate is a pure function of its inputs, so we fuzz the
//! whole type surface with `proptest` rather than relying on hand-picked
//! examples:
//!
//!   * `json_roundtrip_preserves_config` — any `DeviceConfig` survives
//!     serialize → deserialize unchanged (wire compatibility).
//!   * `valid_config_passes_validation` — every config drawn from the
//!     constrained strategy satisfies `DeviceConfig::validate`.
//!   * `corrupted_public_key_fails_validation` — flipping one byte of a
//!     generated key makes validation fail, for any config shape.
//!   * `version_bump_saturates` / `version_order_determines_strictly_newer` —
//!     optimistic-locking arithmetic never overflows and never decreases.

use base64::Engine as _;
use proptest::prelude::*;
use sdwan_core::{
    ConfigVersion, DeviceConfig, DeviceId, FirewallAction, FirewallPolicy, FirewallRule,
    HealthCheckConfig, Interface, OrgId, PathLabel, PathLabelKind, ProbeType, QosClass, QosPolicy,
    Route, SiteId, TunnelConfig, WireGuardTunnel,
};
use std::net::IpAddr;
use uuid::Uuid;

fn uuid_strategy() -> impl Strategy<Value = Uuid> {
    any::<[u8; 16]>().prop_map(Uuid::from_bytes)
}

/// Non-empty lowercase identifier (interface names, hostnames, labels).
fn name_strategy() -> impl Strategy<Value = String> {
    prop::collection::vec(prop::char::range('a', 'z'), 1..=12)
        .prop_map(|v| v.into_iter().collect())
}

/// Base64 of 32 random bytes — exactly the 44-char X25519 public-key shape.
fn wg_key_strategy() -> impl Strategy<Value = String> {
    prop::collection::vec(any::<u8>(), 32).prop_map(|b| {
        base64::engine::general_purpose::STANDARD.encode(b)
    })
}

/// Address vector; `valid` filters out unspecified addresses (`0.0.0.0`, `::`)
/// which `DeviceConfig::validate` rejects.
fn address_vec_strategy(valid: bool) -> impl Strategy<Value = Vec<IpAddr>> {
    let addr = any::<IpAddr>();
    let addr = if valid {
        addr.prop_filter("unspecified address", |a| !a.is_unspecified())
            .boxed()
    } else {
        addr.boxed()
    };
    prop::collection::vec(addr, 0..=4)
}

/// Firewall port; `valid` excludes 0 which `validate` rejects.
fn port_strategy(valid: bool) -> impl Strategy<Value = Option<u16>> {
    if valid {
        prop::option::of(1..=65535u16).boxed()
    } else {
        prop::option::of(any::<u16>()).boxed()
    }
}

/// QoS DSCP; `valid` stays within 0..=63 which `validate` enforces.
fn dscp_strategy(valid: bool) -> impl Strategy<Value = u8> {
    if valid {
        (0..=63u8).boxed()
    } else {
        any::<u8>().boxed()
    }
}

fn interface_strategy(valid: bool) -> impl Strategy<Value = Interface> {
    (
        name_strategy(),
        address_vec_strategy(valid),
        any::<u16>(),
        prop::option::of(name_strategy()),
    )
        .prop_map(|(name, addresses, mtu, path_label)| Interface {
            name,
            addresses,
            mtu,
            path_label,
        })
}

fn health_check_strategy() -> impl Strategy<Value = HealthCheckConfig> {
    (any::<u32>(), 0..4u8, any::<u32>(), any::<u32>()).prop_map(
        |(interval_ms, probe, threshold, timeout_ms)| HealthCheckConfig {
            interval_ms,
            probe_type: match probe {
                0 => ProbeType::Icmp,
                1 => ProbeType::Http,
                2 => ProbeType::Dns,
                _ => ProbeType::Tcp,
            },
            threshold,
            timeout_ms,
        },
    )
}

fn tunnel_strategy() -> impl Strategy<Value = TunnelConfig> {
    (
        name_strategy(),
        name_strategy(),
        health_check_strategy(),
        name_strategy(),
        address_vec_strategy(false),
        wg_key_strategy(),
    )
        .prop_map(
            |(interface, path_label, health_check, endpoint, allowed_ips, public_key)| {
                TunnelConfig::WireGuard(WireGuardTunnel {
                    interface,
                    path_label,
                    health_check,
                    endpoint,
                    allowed_ips,
                    public_key,
                })
            },
        )
}

fn route_strategy() -> impl Strategy<Value = Route> {
    (name_strategy(), name_strategy(), any::<u32>()).prop_map(
        |(destination, next_hop, metric)| Route {
            destination,
            next_hop,
            metric,
        },
    )
}

fn firewall_rule_strategy(valid: bool) -> impl Strategy<Value = FirewallRule> {
    (
        0..3u8,
        prop::option::of(name_strategy()),
        prop::option::of(name_strategy()),
        prop::option::of(name_strategy()),
        port_strategy(valid),
        prop::option::of(name_strategy()),
    )
        .prop_map(
            |(action, source, destination, protocol, port, comment)| FirewallRule {
                action: match action {
                    0 => FirewallAction::Accept,
                    1 => FirewallAction::Drop,
                    _ => FirewallAction::Reject,
                },
                source,
                destination,
                protocol,
                port,
                comment,
            },
        )
}

fn qos_class_strategy(valid: bool) -> impl Strategy<Value = QosClass> {
    (name_strategy(), dscp_strategy(valid), any::<u64>()).prop_map(
        |(name, dscp, bandwidth_bps)| QosClass {
            name,
            dscp,
            bandwidth_bps,
        },
    )
}

fn path_label_strategy() -> impl Strategy<Value = PathLabel> {
    (uuid_strategy(), name_strategy(), 0..6u8, name_strategy()).prop_map(
        |(id, name, kind, sla)| PathLabel {
            id,
            name,
            kind: match kind {
                0 => PathLabelKind::Mpls,
                1 => PathLabelKind::Internet,
                2 => PathLabelKind::FiveG,
                3 => PathLabelKind::Starlink,
                4 => PathLabelKind::Lte,
                _ => PathLabelKind::Other,
            },
            sla,
        },
    )
}

/// Any `DeviceConfig` — deliberately unconstrained fields so roundtrip fuzzing
/// exercises the full value space (including values `validate` would reject).
pub fn any_config() -> impl Strategy<Value = DeviceConfig> {
    (
        uuid_strategy(),
        uuid_strategy(),
        uuid_strategy(),
        name_strategy(),
        prop::collection::vec(interface_strategy(false), 0..=4),
        prop::collection::vec(tunnel_strategy(), 0..=3),
        prop::collection::vec(route_strategy(), 0..=4),
        prop::collection::vec(firewall_rule_strategy(false), 0..=4),
        prop::collection::vec(qos_class_strategy(false), 0..=4),
        prop::collection::vec(path_label_strategy(), 0..=4),
        any::<u64>(),
    )
        .prop_map(
            |(
                device_id,
                org_id,
                site_id,
                hostname,
                interfaces,
                tunnels,
                routes,
                firewall_rules,
                qos_classes,
                path_labels,
                version,
            )| DeviceConfig {
                device_id: DeviceId::from_uuid(device_id),
                org_id: OrgId::from_uuid(org_id),
                site_id: SiteId::from_uuid(site_id),
                hostname,
                interfaces,
                tunnels,
                routes,
                firewall: FirewallPolicy {
                    rules: firewall_rules,
                },
                qos: QosPolicy {
                    classes: qos_classes,
                },
                path_labels,
                version: ConfigVersion::new(version),
            },
        )
}

/// Constrained `DeviceConfig` — every field satisfies the invariants
/// `DeviceConfig::validate` enforces, so the strategy can generate configs
/// that must pass validation (positive property) and that can be corrupted
/// one field at a time (negative property).
pub fn valid_config() -> impl Strategy<Value = DeviceConfig> {
    (
        uuid_strategy(),
        uuid_strategy(),
        uuid_strategy(),
        name_strategy(),
        prop::collection::vec(interface_strategy(true), 0..=4),
        prop::collection::vec(tunnel_strategy(), 0..=3),
        prop::collection::vec(route_strategy(), 0..=4),
        prop::collection::vec(firewall_rule_strategy(true), 0..=4),
        prop::collection::vec(qos_class_strategy(true), 0..=4),
        prop::collection::vec(path_label_strategy(), 0..=4),
        any::<u64>(),
    )
        .prop_map(
            |(
                device_id,
                org_id,
                site_id,
                hostname,
                interfaces,
                tunnels,
                routes,
                firewall_rules,
                qos_classes,
                path_labels,
                version,
            )| DeviceConfig {
                device_id: DeviceId::from_uuid(device_id),
                org_id: OrgId::from_uuid(org_id),
                site_id: SiteId::from_uuid(site_id),
                hostname,
                interfaces,
                tunnels,
                routes,
                firewall: FirewallPolicy {
                    rules: firewall_rules,
                },
                qos: QosPolicy {
                    classes: qos_classes,
                },
                path_labels,
                version: ConfigVersion::new(version),
            },
        )
}

proptest! {
    /// The wire contract: serialize → deserialize is the identity for ANY
    /// generated config, valid or not.
    #[test]
    fn json_roundtrip_preserves_config(cfg in any_config()) {
        let j = serde_json::to_string(&cfg).unwrap();
        let back: DeviceConfig = serde_json::from_str(&j).unwrap();
        prop_assert_eq!(back, cfg);
    }

    /// Every config the constrained strategy can produce must pass validation.
    #[test]
    fn valid_config_passes_validation(cfg in valid_config()) {
        prop_assert!(cfg.validate().is_ok(), "validate rejected a generated config");
    }

    /// Corrupting a generated public key (first byte → '!', outside the base64
    /// alphabet) must make validation fail, for any otherwise-valid config.
    #[test]
    fn corrupted_public_key_fails_validation(
        mut cfg in valid_config().prop_filter("needs a tunnel", |c| !c.tunnels.is_empty()),
    ) {
        let Some(TunnelConfig::WireGuard(w)) = cfg.tunnels.first_mut() else {
            unreachable!("prop_filter guarantees at least one tunnel");
        };
        w.public_key.replace_range(0..1, "!");
        prop_assert!(cfg.validate().is_err(), "corrupted key passed validation");
    }

    /// Version bumps are monotonic and saturate at u64::MAX instead of
    /// overflowing (optimistic-locking arithmetic must never wrap).
    #[test]
    fn version_bump_saturates(mut cfg in any_config(), v in any::<u64>()) {
        cfg.version = ConfigVersion::new(v);
        let c1 = cfg.clone().with_bumped_version();
        let c2 = c1.clone().with_bumped_version();
        prop_assert!(c1.version >= cfg.version);
        prop_assert!(c2.version >= c1.version);
        prop_assert_eq!(c1.version.as_u64(), v.saturating_add(1));
    }

    /// `is_strictly_newer_than` is exactly the version ordering — the
    /// optimistic-locking refusal is sound for every pair of configs.
    #[test]
    fn version_order_determines_strictly_newer(a in any_config(), b in any_config()) {
        if b.version > a.version {
            prop_assert!(b.is_strictly_newer_than(&a));
        } else {
            prop_assert!(!b.is_strictly_newer_than(&a));
        }
    }
}
