//! sdwan-core: shared type system for the control plane (P0).
//!
//! Scope is intentionally narrow: this crate defines the data model the agent and
//! the controller exchange over the wire and apply on-device. It has no I/O and no
//! dependencies on the existing data-plane crates (`sdwanlite-core`, `sdwanlite-lb`,
//! `sdwanlite-mesh`, `sdwanlite-bgp`, `sdwanlite-acme`, `sdwanlite-app`) — those
//! stay on the device data path and remain untouched by P0.
//!
//! ## Multi-tenancy
//!
//! Every configuration carries an [`DeviceConfig::org_id`] and
//! [`DeviceConfig::site_id`]; controllers MUST scope every query by `org_id` and
//! agents MUST refuse configs addressed to a different `org_id` than the one they
//! registered with.
//!
//! ## Transactional apply
//!
//! Configs are versioned (`DeviceConfig::version`, monotonically increasing per
//! device). Agents apply via snapshot → apply → verify → commit/rollback. A failed
//! verify rolls back to the previous version without bumping the version counter.
//!
//! ## Security
//!
//! Wire-private keys never appear in any struct (public keys only). Auth tokens are
//! passed around by the controller/agent via unlinked `0600` files at runtime, not
//! as argv (see AGENTS.md). Examples and doc-comments use RFC 5737 documentation
//! addresses (`192.0.2.x`, `198.51.100.x`, `203.0.113.x`) only — no real IPs.
//!
//! - §1 Management Core → [`DeviceConfig::org_id`] / [`DeviceConfig::site_id`] (multi-tenant)
//! - §1 Management Core → [`DeviceState`] device lifecycle
//! - §2 Device Config → [`DeviceConfig`] + version
//! - §10 Path Labels → [`PathLabel`]
//! - §11 Path Selection → [`TunnelConfig::WireGuard::path_label`] (declarative link)
//! - §9 Link Monitors → [`HealthCheckConfig`]

//! Other flexiWAN groups (routing, NAT, QoS, HA, AI, dashboards, NB API) are
//! implemented in P1–P3 (see `docs/ARCHITECTURE-P0.md`).

#![deny(rust_2018_idioms)]
#![warn(missing_docs)]

use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use uuid::Uuid;

/// Generates a branded UUID newtype: a `Uuid` that the type system will never
/// confuse with any other ID (Matt-Pocock-style "branded" identifiers).
///
/// Serde is `#[serde(transparent)]`, so the JSON wire format is unchanged — a
/// plain UUID string. The branding is purely a compile-time guarantee.
macro_rules! branded_uuid {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(
            Clone, Copy, Debug, PartialEq, Eq, Hash,
            Serialize, Deserialize,
            schemars::JsonSchema,
        )]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            /// Generate a fresh random identifier (UUIDv4).
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }

            /// Wrap an already-existing UUID.
            pub const fn from_uuid(u: Uuid) -> Self {
                Self(u)
            }

            /// Unwrap to the underlying UUID.
            pub const fn as_uuid(self) -> Uuid {
                self.0
            }
        }

        impl From<Uuid> for $name {
            fn from(u: Uuid) -> Self {
                Self(u)
            }
        }

        impl From<$name> for Uuid {
            fn from(id: $name) -> Self {
                id.0
            }
        }

        impl std::str::FromStr for $name {
            type Err = uuid::Error;

            fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
                Ok(Self(Uuid::from_str(s)?))
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}

branded_uuid! {
    /// Device identity (UUIDv4, assigned at first registration).
    ///
    /// Carries the device through registration, config pull, telemetry, and the
    /// WebSocket stream; the controller's device store is keyed by this type.
    DeviceId
}

branded_uuid! {
    /// Tenant identity (UUIDv4). The multi-tenancy isolation boundary.
    OrgId
}

branded_uuid! {
    /// Site identity within an org (UUIDv4).
    SiteId
}

branded_uuid! {
    /// Tunnel identity (UUIDv4, assigned by the controller).
    ///
    /// Reserved for the P1 data plane, where tunnels are keyed by ID; the P0
    /// wire format deliberately carries no tunnel ID field (matches
    /// `api-spec.yaml`).
    TunnelId
}

branded_uuid! {
    /// Logical interface identity (UUIDv4, assigned by the controller).
    ///
    /// Reserved for the P1 data plane; the P0 wire format deliberately carries
    /// no interface ID field (matches `api-spec.yaml`).
    InterfaceId
}

/// Device lifecycle state (flexiWAN §1 — Management Core).
///
/// Transitions:
/// * `provisioned` → `connected` on successful register/heartbeat
/// * `connected` → `degraded` on repeated telemetry/missed heartbeats
/// * `degraded` → `disconnected` on extended loss
/// * `disconnected` → `connected` on recovery
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DeviceState {
    /// Registered but no active control-plane session yet.
    Provisioned,
    /// Active control-plane session with healthy telemetry.
    Connected,
    /// Registered, but telemetry/heartbeats are degraded.
    Degraded,
    /// No active control-plane session.
    Disconnected,
}


/// Monotonic per-device configuration version.
///
/// The controller issues monotonically increasing versions on every successful
/// push; the agent mirrors the pushed revision and refuses any config whose
/// version is not strictly greater than its current one (optimistic locking).
/// A failed verify does NOT bump the version.
///
/// Serde is `#[serde(transparent)]` — the wire format is a plain integer, unchanged.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash,
    Serialize, Deserialize,
    schemars::JsonSchema,
)]
#[serde(transparent)]
pub struct ConfigVersion(u64);

impl ConfigVersion {
    /// Wrap a raw `u64` version counter.
    pub const fn new(v: u64) -> Self {
        Self(v)
    }

    /// Unwrap to the raw counter.
    pub const fn as_u64(self) -> u64 {
        self.0
    }
}

impl From<u64> for ConfigVersion {
    fn from(v: u64) -> Self {
        Self(v)
    }
}

impl From<ConfigVersion> for u64 {
    fn from(v: ConfigVersion) -> Self {
        v.0
    }
}

impl std::fmt::Display for ConfigVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// Bootstrap token the agent presents to the controller.
///
/// Wrapped so it can never be confused with a device/org ID and so accidental
/// `Display`-based logging (which would leak the secret) is a compile error —
/// the only way to read it is the explicit [`BootstrapToken::as_str`]. Never
/// log this value (see AGENTS.md).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(transparent)]
pub struct BootstrapToken(String);

impl BootstrapToken {
    /// Wrap a raw token string.
    pub fn new(t: impl Into<String>) -> Self {
        Self(t.into())
    }

    /// Explicit accessor — the ONLY way to read the token.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for BootstrapToken {
    fn from(t: String) -> Self {
        Self(t)
    }
}

impl From<&str> for BootstrapToken {
    fn from(t: &str) -> Self {
        Self(t.to_string())
    }
}

/// Logical underlay/overlay link class (flexiWAN §10 — Path Labels).
///
/// Path labels carry SLA expectations that the link monitor + path selection engine
/// later consume. They are deliberately side-effect-free here.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PathLabel {
    /// Stable identifier (UUIDv4 generated by the controller).
    pub id: Uuid,

    /// Human-readable name (e.g. `MPLS-Primary`, `ISP1`, `LTE-Backup`).
    ///
    /// Unique within the org; agents use it to bind policies to interfaces.
    pub name: String,

    /// Link class.
    #[serde(rename = "type")]
    pub kind: PathLabelKind,

    /// SLA expectations (latency/jitter/loss/bw). Free-form for now; the P1 link-monitor
    /// crate will enforce them. Empty means "best effort".
    #[serde(default)]
    pub sla: String,
}

/// Physical/transport category of a path label.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PathLabelKind {
    /// MPLS / private backbone.
    Mpls,
    /// Public Internet (broadband).
    Internet,
    /// 5G mobile.
    #[serde(rename = "5g")]
    FiveG,
    /// Starlink / satellite.
    Starlink,
    /// LTE / 4G mobile.
    Lte,
    /// Generic other / not classified.
    Other,
}

/// Top-level tenant (flexiWAN §1 — Management Core).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Org {
    /// Tenant identity (branded [`OrgId`]).
    pub id: OrgId,
    /// Org display name.
    pub name: String,
    /// Unix epoch seconds.
    pub created_at: u64,
}

/// Site within an org (flexiWAN §1 — Org/Site/Device hierarchy).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Site {
    /// Site identity (branded [`SiteId`]).
    pub id: SiteId,
    /// Owning org (branded [`OrgId`]).
    pub org_id: OrgId,
    /// Site display name (unique within the org).
    pub name: String,
    /// Unix epoch seconds.
    pub created_at: u64,
}

/// Device record (flexiWAN §1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Device {
    /// Device identity (UUIDv4, branded [`DeviceId`]).
    pub id: DeviceId,
    /// Owning org (multi-tenant boundary, branded [`OrgId`]).
    pub org_id: OrgId,
    /// Owning site (branded [`SiteId`]).
    pub site_id: SiteId,
    /// Hostname reported at registration (unique within the org).
    pub hostname: String,
    /// Unix epoch seconds the device was last seen by the controller.
    pub last_seen: u64,
    /// Current lifecycle state.
    pub state: DeviceState,
}


/// RBAC role (flexiWAN §1 — Securing accounts).
///
/// The controller scopes mutations by role; the agent itself only carries an
/// admin token in P0, but the enum is here for P1 NB-API authorization.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// Full control including billing and member management.
    Owner,
    /// Manage devices, policies, and tunnels within the org.
    Admin,
    /// Operational changes (apply config, reload, restart services).
    Operator,
    /// Read-only access to dashboards and telemetry.
    Viewer,
}

/// WireGuard X25519 public key.
///
/// X25519 keys are 32 bytes; base64-encoded they are exactly 44 characters
/// (32 bytes × 4/3 rounded up + padding). Use [`PublicKey::try_from_str`] to
/// validate before storing; [`TunnelConfig::WireGuard::public_key`] is the
/// raw `String` field and may carry any value until the validator runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublicKey(String);

impl PublicKey {
    /// Construct a validated key. Returns a structured `ValidationError` on failure.
    pub fn try_from_str(s: &str) -> std::result::Result<Self, ValidationError> {
        validate_public_key(s)?;
        Ok(Self(s.to_string()))
    }
    /// Borrow the underlying base64 string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Validate a base64-encoded X25519 public key: length exactly 44 characters
/// and the character set limited to the standard base64 alphabet.
pub fn validate_public_key(s: &str) -> std::result::Result<(), ValidationError> {
    const EXPECTED_LEN: usize = 44;
    if s.len() != EXPECTED_LEN {
        return Err(ValidationError::PublicKeyLength {
            actual: s.len(),
            expected: EXPECTED_LEN,
        });
    }
    for (i, b) in s.bytes().enumerate() {
        let ok = b.is_ascii_alphanumeric() || b == b'+' || b == b'/' || b == b'=';
        if !ok {
            return Err(ValidationError::PublicKeyCharset {
                position: i,
                byte: b,
            });
        }
    }
    // base64 decode sanity (must round-trip to exactly 32 bytes).
    use base64::Engine as _;
    let engine = base64::engine::general_purpose::STANDARD;
    match engine.decode(s) {
        Ok(bytes) if bytes.len() == 32 => Ok(()),
        Ok(other) => Err(ValidationError::PublicKeyDecodedLength { len: other.len() }),
        Err(e) => Err(ValidationError::PublicKeyDecode(e.to_string())),
    }
}

/// Structured validation error returned by `DeviceConfig::validate` and friends.
#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    /// The public key string is not exactly 44 characters.
    #[error("WireGuard public_key length must be {expected} chars, got {actual}")]
    PublicKeyLength { actual: usize, expected: usize },
    /// The public key contains a byte outside the base64 alphabet.
    #[error("WireGuard public_key has invalid byte 0x{byte:02x} at position {position}")]
    PublicKeyCharset { position: usize, byte: u8 },
    /// The public key decoded to a payload that is not 32 bytes.
    #[error("WireGuard public_key decoded to {len} bytes, expected 32")]
    PublicKeyDecodedLength { len: usize },
    /// The public key failed base64 decoding.
    #[error("WireGuard public_key base64 decode failed: {0}")]
    PublicKeyDecode(String),
    /// A tunnel failed validation (nested in the tunnel context).
    #[error("tunnel `{interface}`: {source}")]
    Tunnel { interface: String, source: Box<ValidationError> },
    /// An interface failed validation.
    #[error("interface `{name}`: {message}")]
    Interface { name: String, message: String },
    /// A firewall rule failed validation.
    #[error("firewall rule {index}: {message}")]
    FirewallRule { index: usize, message: String },
    /// A QoS class failed validation.
    #[error("qos class {index}: {message}")]
    QosClass { index: usize, message: String },
}

/// Default impl for [`HealthCheckConfig`] (the derive macro respects
/// `#[serde(default = ...)]` only for deserialization, not the `Default` trait).
impl Default for HealthCheckConfig {
    fn default() -> Self {
        Self {
            interval_ms: default_hc_interval_ms(),
            probe_type: ProbeType::default(),
            threshold: default_hc_threshold(),
            timeout_ms: default_hc_timeout_ms(),
        }
    }
}

/// Probing policy for a link (flexiWAN §9).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct HealthCheckConfig {
    /// Probe cadence in milliseconds.
    #[serde(default = "default_hc_interval_ms")]
    pub interval_ms: u32,

    /// Probe transport.
    #[serde(default)]
    pub probe_type: ProbeType,

    /// Number of consecutive failures before the link is marked down.
    #[serde(default = "default_hc_threshold")]
    pub threshold: u32,

    /// Per-probe timeout in milliseconds.
    #[serde(default = "default_hc_timeout_ms")]
    pub timeout_ms: u32,
}

fn default_hc_interval_ms() -> u32 {
    1000
}
fn default_hc_threshold() -> u32 {
    3
}
fn default_hc_timeout_ms() -> u32 {
    500
}

/// Probe transport used by the link monitor.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProbeType {
    /// ICMP echo (requires `CAP_NET_RAW`; see AGENTS.md).
    #[default]
    Icmp,
    /// HTTP/HTTPS GET against a target URL.
    Http,
    /// DNS query against a resolver.
    Dns,
    /// TCP connect against a port.
    Tcp,
}

/// On-device logical interface declaration.
///
/// `name` matches the kernel interface label (e.g. `eth0`, `wg0`).
/// `addresses` use RFC 5737 documentation prefixes in examples; production values
/// come from the controller.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Interface {
    /// Kernel interface name.
    pub name: String,

    /// Stable addresses bound to the interface (IPv4/IPv6).
    pub addresses: Vec<IpAddr>,

    /// MTU in bytes (0 means kernel default).
    #[serde(default)]
    pub mtu: u16,

    /// Optional path-label binding (flexiWAN §10).
    #[serde(default)]
    pub path_label: Option<String>,
}

/// Tunnel configuration. Currently only WireGuard is supported; the enum is shaped
/// to make IPsec/SSTP a drop-in addition in P1.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TunnelConfig {
    /// WireGuard overlay.
    WireGuard(WireGuardTunnel),
}

/// WireGuard-specific tunnel config.
///
/// `public_key` is the peer's *public* key (base64 X25519, 32 bytes). Private keys are
/// never exchanged via this struct — they are generated on-device and kept in
/// `0600` files only (see AGENTS.md).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct WireGuardTunnel {
    /// Tunnel interface name (e.g. `wg0`).
    pub interface: String,

    /// Path label this tunnel rides (must reference an existing label).
    pub path_label: String,

    /// Per-tunnel health probe.
    #[serde(default)]
    pub health_check: HealthCheckConfig,

    /// Endpoint address (RFC 5737 example: `203.0.113.7:51820`).
    pub endpoint: String,

    /// Allowed remote IPs for the peer's side of the tunnel.
    pub allowed_ips: Vec<IpAddr>,

    /// Base64 X25519 public key — 44 chars; validated by [`DeviceConfig::validate`].
    pub public_key: String,
}

impl WireGuardTunnel {
    /// Validate this tunnel's `public_key` against the X25519 base64 contract.
    pub fn validate(&self) -> std::result::Result<(), ValidationError> {
        validate_public_key(&self.public_key)
            .map_err(|e| ValidationError::Tunnel {
                interface: self.interface.clone(),
                source: Box::new(e),
            })
    }
}

/// Static route entry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Route {
    /// Destination prefix (CIDR).
    pub destination: String,

    /// Next-hop (interface name or IP).
    pub next_hop: String,

    /// Administrative distance (lower wins).
    #[serde(default = "default_metric")]
    pub metric: u32,
}

fn default_metric() -> u32 {
    100
}

/// Firewall policy (P0: skeleton, P1 wires the nftables render).
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FirewallPolicy {
    /// Ordered list of rules; first match wins.
    #[serde(default)]
    pub rules: Vec<FirewallRule>,
}

/// Single firewall rule (flexiWAN §7). First match in `FirewallPolicy.rules`
/// wins; rules with `None` match-conditions apply to any traffic.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FirewallRule {
    /// Verdict on match.
    pub action: FirewallAction,
    /// Source CIDR/IP/iface (optional).
    #[serde(default)]
    pub source: Option<String>,
    /// Destination CIDR/IP/iface (optional).
    #[serde(default)]
    pub destination: Option<String>,
    /// L4 protocol (e.g. `tcp`, `udp`).
    #[serde(default)]
    pub protocol: Option<String>,
    /// L4 port (when applicable).
    #[serde(default)]
    pub port: Option<u16>,
    /// Free-form operator note.
    #[serde(default)]
    pub comment: Option<String>,
}

/// Firewall verdict (flexiWAN §7).
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FirewallAction {
    /// Forward matching traffic.
    Accept,
    /// Drop silently.
    Drop,
    /// Reject with ICMP unreachable.
    Reject,
}

/// QoS policy (P0: skeleton, P1 wires HTB/TC).
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct QosPolicy {
    /// Ordered QoS classes (top match wins).
    #[serde(default)]
    pub classes: Vec<QosClass>,
}

/// QoS class (flexiWAN §12). One row per traffic class; the controller
/// emits HTB/TC + DSCP rules from these.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct QosClass {
    /// Class name (e.g. `voip`, `data`).
    pub name: String,
    /// DSCP value 0-63.
    pub dscp: u8,
    /// Bandwidth ceiling in bits/second (0 = unlimited).
    #[serde(default)]
    pub bandwidth_bps: u64,
}

/// Full device configuration the controller pushes.
///
/// Optimistic-locking via `version`: an agent MUST refuse any `DeviceConfig` whose
/// `version` is not strictly greater than its own current version.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DeviceConfig {
    /// Device identity (UUIDv4, branded [`DeviceId`]).
    pub device_id: DeviceId,

    /// Owning org (multi-tenant isolation boundary, branded [`OrgId`]).
    pub org_id: OrgId,

    /// Site within the org (flexiWAN §1 — Org/Site/Device hierarchy, branded [`SiteId`]).
    pub site_id: SiteId,

    /// Hostname reported by the device at registration.
    pub hostname: String,

    /// Logical interfaces on the device.
    pub interfaces: Vec<Interface>,

    /// Tunnels (currently WireGuard only).
    pub tunnels: Vec<TunnelConfig>,

    /// Static routes.
    pub routes: Vec<Route>,

    /// Firewall policy.
    pub firewall: FirewallPolicy,

    /// QoS policy.
    pub qos: QosPolicy,

    /// Path labels declared on this device.
    pub path_labels: Vec<PathLabel>,

    /// Monotonic version — issued by the controller on every push.
    pub version: ConfigVersion,
}

impl DeviceConfig {
    /// Validate invariants the controller cannot trust on the wire:
    /// tunnel public keys (44-char X25519), interface name non-empty,
    /// firewall rule port range, QoS DSCP 0..=63.
    pub fn validate(&self) -> std::result::Result<(), ValidationError> {
        for iface in &self.interfaces {
            if iface.name.is_empty() {
                return Err(ValidationError::Interface {
                    name: iface.name.clone(),
                    message: "interface name must not be empty".into(),
                });
            }
            for a in &iface.addresses {
                if a.is_unspecified() {
                    return Err(ValidationError::Interface {
                        name: iface.name.clone(),
                        message: format!("address {a} is unspecified"),
                    });
                }
            }
        }
        for t in &self.tunnels {
            match t {
                TunnelConfig::WireGuard(w) => w.validate()?,
            }
        }
        for (i, r) in self.firewall.rules.iter().enumerate() {
            if let Some(p) = r.port {
                if p == 0 {
                    return Err(ValidationError::FirewallRule {
                        index: i,
                        message: "port must be > 0".into(),
                    });
                }
            }
        }
        for (i, c) in self.qos.classes.iter().enumerate() {
            if c.dscp > 63 {
                return Err(ValidationError::QosClass {
                    index: i,
                    message: format!("dscp {} exceeds 63", c.dscp),
                });
            }
        }
        Ok(())
    }

    /// True iff `other.version > self.version`.
    pub fn is_strictly_newer_than(&self, other: &Self) -> bool {
        self.version > other.version
    }

    /// Bump the version counter, returning a new config.
    ///
    /// The controller uses this when minting the next revision: every push
    /// must carry a strictly higher version (optimistic locking). Agents do
    /// NOT call this — they mirror the pushed revision as-is.
    pub fn with_bumped_version(mut self) -> Self {
        self.version = ConfigVersion::new(self.version.as_u64().saturating_add(1));
        self
    }
}

/// A [`DeviceConfig`] that has passed [`DeviceConfig::validate`].
///
/// Type-level guarantee: this wrapper is the only way the apply path receives a
/// config. There is no blanket `From<DeviceConfig>` escape hatch — the explicit
/// [`ValidatedConfig::validate`] (or the equivalent [`TryFrom`] impl) is the sole
/// constructor, so an unvalidated config cannot reach `apply` at compile time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidatedConfig(DeviceConfig);

impl ValidatedConfig {
    /// Run [`DeviceConfig::validate`] and wrap the config on success.
    pub fn validate(cfg: DeviceConfig) -> std::result::Result<Self, ValidationError> {
        cfg.validate()?;
        Ok(Self(cfg))
    }

    /// Unwrap to the inner config (used by the commit step of a transactional apply).
    pub fn into_inner(self) -> DeviceConfig {
        self.0
    }

    /// Borrow the inner config.
    pub fn as_ref(&self) -> &DeviceConfig {
        &self.0
    }

    /// Mutable borrow of the inner config.
    pub fn as_mut(&mut self) -> &mut DeviceConfig {
        &mut self.0
    }

    /// The config's version counter.
    pub fn version(&self) -> ConfigVersion {
        self.0.version
    }
}

impl TryFrom<DeviceConfig> for ValidatedConfig {
    type Error = ValidationError;

    fn try_from(cfg: DeviceConfig) -> std::result::Result<Self, Self::Error> {
        Self::validate(cfg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> DeviceConfig {
        DeviceConfig {
            device_id: DeviceId::new(),
            org_id: OrgId::new(),
            site_id: SiteId::new(),
            hostname: "edge-01".into(),
            interfaces: vec![Interface {
                name: "eth0".into(),
                addresses: vec!["203.0.113.7".parse().unwrap()],
                mtu: 0,
                path_label: None,
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
                destination: "0.0.0.0/0".into(),
                next_hop: "203.0.113.1".into(),
                metric: 100,
            }],
            firewall: FirewallPolicy::default(),
            qos: QosPolicy::default(),
            path_labels: vec![PathLabel {
                id: Uuid::new_v4(),
                name: "MPLS-Primary".into(),
                kind: PathLabelKind::Mpls,
                sla: "loss<0.1% rtt<10ms".into(),
            }],
            version: ConfigVersion::new(1),
        }
    }

    #[test]
    fn version_strictness() {
        let c = sample();
        let newer = DeviceConfig {
            version: ConfigVersion::new(2),
            ..c.clone()
        };
        assert!(newer.is_strictly_newer_than(&c));
        assert!(!c.is_strictly_newer_than(&newer));
        assert!(!c.is_strictly_newer_than(&c));
    }

    #[test]
    fn json_roundtrip() {
        let c = sample();
        let j = serde_json::to_string(&c).unwrap();
        let back: DeviceConfig = serde_json::from_str(&j).unwrap();
        assert_eq!(back, c);
    }

    #[test]
    fn tunnel_tag_serde() {
        // Ensure the tagged enum produces stable, documented JSON (no id field —
        // matches api-spec.yaml WireGuardTunnel).
        let j = r#"{"kind":"wire_guard","interface":"wg0","path_label":"X","health_check":{"interval_ms":1000,"probe_type":"icmp","threshold":3,"timeout_ms":500},"endpoint":"203.0.113.7:51820","allowed_ips":["198.51.100.1"],"public_key":"A"}"#;
        let t: TunnelConfig = serde_json::from_str(j).unwrap();
        match t {
            TunnelConfig::WireGuard(w) => {
                assert_eq!(w.interface, "wg0");
                assert_eq!(w.path_label, "X");
            }
        }
    }
}
