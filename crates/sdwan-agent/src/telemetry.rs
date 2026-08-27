//! Telemetry snapshot the agent pushes to the controller every 10s (configurable).
//!
//! Endpoints in this struct use RFC 5737 documentation addresses (`192.0.2.x`,
//! `198.51.100.x`, `203.0.113.x`) in examples; production values come from the device
//! data plane via the existing `sdwanlite-lb` and `sdwanlite-bgp` crates. The agent
//! is the bridge; it does NOT collect raw metrics itself (flexiWAN §16 — Dashboards
//! belongs to P3).

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// One telemetry frame the agent POSTs to `/api/v1/telemetry`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TelemetryFrame {
    /// Device identity (must match the registered device).
    pub device_id: Uuid,

    /// Owning org (multi-tenant scope).
    pub org_id: Uuid,

    /// Device uptime in seconds.
    pub uptime_secs: u64,

    /// Per-link load snapshot.
    pub links: Vec<LinkSample>,

    /// Free-form health flags from the data plane.
    #[serde(default)]
    pub flags: Vec<HealthFlag>,
}

/// Load sample for one logical link.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkSample {
    /// Path-label name (e.g. `MPLS-Primary`).
    pub path_label: String,

    /// Interface name (e.g. `wg0`, `eth0`).
    pub interface: String,

    /// Local endpoint (RFC 5737 example: `203.0.113.7:51820`).
    pub local_endpoint: String,

    /// TX bytes since last reset (monotonic on device).
    pub tx_bytes: u64,

    /// RX bytes since last reset.
    pub rx_bytes: u64,

    /// Peer endpoint, if known.
    #[serde(default)]
    pub peer_endpoint: Option<String>,
}

/// Health flag surfaced by the data plane.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HealthFlag {
    /// Link is down (probe failures exceeded threshold).
    LinkDown { path_label: String },
    /// Degraded mode (some sibling subsystem missing its JSON).
    Degraded { subsystem: String },
}

/// Empty helper for the `links` list.
#[allow(dead_code)]
pub fn empty_frame(device_id: Uuid, org_id: Uuid) -> TelemetryFrame {
    TelemetryFrame {
        device_id,
        org_id,
        uptime_secs: 0,
        links: Vec::new(),
        flags: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_roundtrip() {
        let f = TelemetryFrame {
            device_id: Uuid::new_v4(),
            org_id: Uuid::new_v4(),
            uptime_secs: 42,
            links: vec![LinkSample {
                path_label: "MPLS".into(),
                interface: "wg0".into(),
                local_endpoint: "203.0.113.7:51820".into(),
                tx_bytes: 1234,
                rx_bytes: 5678,
                peer_endpoint: Some(
                    "198.51.100.10:51820"
                        .parse::<std::net::SocketAddr>()
                        .unwrap()
                        .to_string(),
                ),
            }],
            flags: vec![HealthFlag::LinkDown {
                path_label: "LTE".into(),
            }],
        };
        let j = serde_json::to_string(&f).unwrap();
        let back: TelemetryFrame = serde_json::from_str(&j).unwrap();
        assert_eq!(back, f);
    }

}
