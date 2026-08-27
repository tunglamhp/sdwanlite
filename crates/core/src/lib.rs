//! sdwanlite-core: configuration model and shared types.

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("failed to read config file {0}: {1}")]
    Read(String, #[source] std::io::Error),
    #[error("failed to parse config: {0}")]
    Parse(#[from] toml::de::Error),
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct Config {
    #[serde(default)]
    pub general: General,
    #[serde(default)]
    pub mesh: Mesh,
    #[serde(default)]
    pub bgp: Bgp,
    #[serde(default)]
    pub acme: Acme,
    #[serde(default)]
    pub firewall: Vec<FirewallRule>,
    #[serde(default)]
    pub qos: QosLimit,
    #[serde(default)]
    pub lb: LoadBalancers,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct General {
    pub name: String,
    #[serde(default = "default_api_addr")]
    pub api_addr: String,
    #[serde(default = "default_api_port")]
    pub api_port: u16,
    /// Optional bearer token protecting mutating API endpoints.
    #[serde(default)]
    pub api_token: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TlsConfig {
    pub cert_file: String,
    pub key_file: String,
}

/// Let's Encrypt automation (HTTP-01 challenge).
/// The daemon runs a tiny challenge server on `http01_port`, obtains the
/// certificate and writes it to `cert_file`/`key_file`; point an HttpPool's
/// TLS section at the same paths and reload TLS to pick it up.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct Acme {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_acme_directory")]
    pub directory_url: String,
    #[serde(default)]
    pub email: String,
    /// DNS names to include in the certificate.
    #[serde(default)]
    pub domains: Vec<String>,
    #[serde(default = "default_http01_port")]
    pub http01_port: u16,
    #[serde(default)]
    pub cert_file: String,
    #[serde(default)]
    pub key_file: String,
    /// Renew when less than this many days remain (default 30).
    #[serde(default = "default_renew_days")]
    pub renew_days: u32,
    /// Use the DNS-01 challenge instead of HTTP-01. Required for wildcard
    /// names; currently supports Cloudflare via API token.
    #[serde(default)]
    pub dns01: bool,
    /// Cloudflare API token with Zone.DNS Edit permission.
    #[serde(default)]
    pub cloudflare_api_token: Option<String>,
}

fn default_acme_directory() -> String {
    "https://acme-staging-v2.api.letsencrypt.org/directory".into()
}
fn default_http01_port() -> u16 {
    80
}
fn default_renew_days() -> u32 {
    30
}

/// Firewall rule: allow or deny traffic matching port/protocol.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FirewallRule {
    pub action: String, // "allow" | "deny"
    pub port: u16,
    pub protocol: String, // "tcp" | "udp" | "any"
    #[serde(default)]
    pub source: Option<String>, // CIDR or IP
    #[serde(default)]
    pub comment: String,
}

/// QoS bandwidth limit per pool.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct QosLimit {
    /// Max connections per pool (0 = unlimited).
    #[serde(default)]
    pub max_conns: u32,
    /// Max bytes/s per pool (0 = unlimited).
    #[serde(default)]
    pub max_bps: u64,
}

/// Alert event.
#[derive(Clone, Debug, Serialize)]
pub struct AlertEvent {
    pub timestamp: u64,
    pub severity: String, // "info" | "warn" | "critical"
    pub source: String,
    pub message: String,
}

/// Upstream protocol for HTTP pools.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendProto {
    #[default]
    Http1,
    H2,
}

impl Default for General {
    fn default() -> Self {
        Self {
            name: "sdwanlite-node".into(),
            api_addr: default_api_addr(),
            api_port: default_api_port(),
            api_token: None,
        }
    }
}

fn default_api_addr() -> String {
    "127.0.0.1".into()
}
fn default_api_port() -> u16 {
    8080
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct Mesh {
    #[serde(default)]
    pub enabled: bool,
    /// Base64 private key (32 bytes). If empty, a keypair can be generated via the API.
    #[serde(default)]
    pub private_key: String,
    #[serde(default = "default_wg_port")]
    pub listen_port: u16,
    #[serde(default)]
    pub peers: Vec<Peer>,
}

fn default_wg_port() -> u16 {
    51820
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Peer {
    pub name: String,
    pub public_key: String,
    #[serde(default)]
    pub endpoint: Option<String>,
    #[serde(default)]
    pub allowed_ips: Vec<String>,
    /// Persistent keepalive interval in seconds (0 = disabled).
    #[serde(default)]
    pub keepalive_secs: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct Bgp {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub router_id: String,
    #[serde(default = "default_asn")]
    pub local_as: u32,
    #[serde(default = "default_bgp_port")]
    pub listen_port: u16,
    /// Negotiated hold time proposal in seconds.
    #[serde(default = "default_hold_time")]
    pub hold_time_secs: u16,
    #[serde(default)]
    pub neighbors: Vec<BgpNeighbor>,
    /// IPv4 prefixes this node originates.
    #[serde(default)]
    pub networks: Vec<String>,
    /// Optional global import filter: only accept prefixes in this list (exact match).
    #[serde(default)]
    pub import_allowlist: Vec<String>,
    /// Keep multiple equal-cost routes per prefix instead of one best path.
    #[serde(default)]
    pub multipath: bool,
    /// Act as a route reflector for the configured neighbors (lab-grade RR):
    /// routes learned from one neighbor are reflected to the others with
    /// CLUSTER_LIST loop prevention.
    #[serde(default)]
    pub route_reflector: bool,
    /// BGP Identifier of this reflector used in CLUSTER_LIST
    /// (defaults to router_id when empty).
    #[serde(default)]
    pub cluster_id: String,
}

fn default_asn() -> u32 {
    65000
}
fn default_hold_time() -> u16 {
    180
}
fn default_bgp_port() -> u16 {
    179
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BgpNeighbor {
    pub ip: String,
    pub remote_as: u32,
    /// Per-neighbor import override; None = fall back to the global list.
    #[serde(default)]
    pub import_allowlist: Option<Vec<String>>,
    /// Prefixes advertised to this neighbor (exact match); None = all.
    #[serde(default)]
    pub export_allowlist: Option<Vec<String>>,
    /// LOCAL_PREF value attached to routes learned from this neighbor
    /// (higher = preferred). Default 100.
    #[serde(default = "default_local_pref")]
    pub local_pref: u32,
}

fn default_local_pref() -> u32 {
    100
}

impl BgpNeighbor {
    pub fn effective_import<'a>(&'a self, global: &'a [String]) -> &'a [String] {
        self.import_allowlist.as_deref().unwrap_or(global)
    }
    pub fn effective_export<'a>(&'a self, global: &'a [String]) -> &'a [String] {
        self.export_allowlist.as_deref().unwrap_or(global)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct LoadBalancers {
    #[serde(default, rename = "tcp")]
    pub tcp_pools: Vec<TcpPool>,
    #[serde(default, rename = "http")]
    pub http_pools: Vec<HttpPool>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TcpPool {
    pub name: String,
    pub listen: String,
    #[serde(default = "default_algo")]
    pub algorithm: Algorithm,
    #[serde(default = "default_hc_interval")]
    pub health_interval_secs: u64,
    #[serde(default = "default_hc_timeout")]
    pub health_timeout_secs: u64,
    /// When set, health checks send `GET <path>` and require a 2xx/3xx answer
    /// instead of a bare TCP connect.
    #[serde(default)]
    pub health_check_path: Option<String>,
    #[serde(default)]
    pub qos: Option<QosLimit>,
    pub backends: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Algorithm {
    RoundRobin,
    LeastConnections,
    Random,
    /// Always use first healthy backend; switch only when it goes down.
    Failover,
}

impl Algorithm {
    pub fn as_str(&self) -> &'static str {
        match self {
            Algorithm::RoundRobin => "round_robin",
            Algorithm::LeastConnections => "least_connections",
            Algorithm::Random => "random",
            Algorithm::Failover => "failover",
        }
    }
}

fn default_algo() -> Algorithm {
    Algorithm::RoundRobin
}
fn default_hc_interval() -> u64 {
    5
}
fn default_hc_timeout() -> u64 {
    2
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HttpPool {
    pub name: String,
    pub listen: String,
    /// Optional TLS termination for this listener.
    #[serde(default)]
    pub tls: Option<TlsConfig>,
    #[serde(default = "default_hc_interval")]
    pub health_interval_secs: u64,
    #[serde(default = "default_hc_timeout")]
    pub health_timeout_secs: u64,
    #[serde(default)]
    pub health_check_path: Option<String>,
    /// Protocol spoken by backends for this pool.
    #[serde(default)]
    pub backend_proto: BackendProto,
    #[serde(default)]
    pub routes: Vec<HttpRoute>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HttpRoute {
    /// Match Host header (without port). Empty string matches any host.
    #[serde(default)]
    pub host: String,
    /// Longest matching path prefix wins. Empty matches everything.
    #[serde(default)]
    pub path_prefix: String,
    pub backends: Vec<String>,
}

impl Config {
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let raw = std::fs::read_to_string(path)
            .map_err(|e| ConfigError::Read(path.display().to_string(), e))?;
        let cfg: Config = toml::from_str(&raw)?;
        Ok(cfg)
    }

    pub fn load_or_sample(path: &Path) -> (Self, bool) {
        match Self::load(path) {
            Ok(c) => (c, false),
            Err(_) => (Self::sample(), true),
        }
    }

    /// A small demo configuration used when no file is provided.
    pub fn sample() -> Self {
        let mut c = Self::default();
        c.lb.tcp_pools.push(TcpPool {
            name: "demo-tcp".into(),
            listen: "127.0.0.1:9000".into(),
            algorithm: Algorithm::LeastConnections,
            health_check_path: None,
            qos: None,
            health_interval_secs: 5,
            health_timeout_secs: 2,
            backends: vec!["127.0.0.1:9101".into(), "127.0.0.1:9102".into()],
        });
        c.lb.http_pools.push(HttpPool {
            name: "demo-http".into(),
            listen: "127.0.0.1:9090".into(),
            tls: None,
            health_check_path: None,
            backend_proto: BackendProto::Http1,
            health_interval_secs: 5,
            health_timeout_secs: 2,
            routes: vec![HttpRoute {
                host: String::new(),
                path_prefix: "/".into(),
                backends: vec!["127.0.0.1:9201".into()],
            }],
        });
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_config() {
        let raw = r#"
[general]
name = "edge-1"
api_port = 9000

[[lb.tcp]]
name = "web"
listen = "0.0.0.0:80"
algorithm = "least_connections"
backends = ["10.0.0.1:80", "10.0.0.2:80"]
"#;
        let cfg: Config = toml::from_str(raw).unwrap();
        assert_eq!(cfg.general.name, "edge-1");
        assert_eq!(cfg.general.api_port, 9000);
        assert_eq!(cfg.lb.tcp_pools[0].algorithm, Algorithm::LeastConnections);
        assert_eq!(cfg.lb.tcp_pools[0].backends.len(), 2);
    }

    #[test]
    fn sample_config_parses() {
        let s = toml::to_string(&Config::sample()).unwrap();
        let cfg: Config = toml::from_str(&s).unwrap();
        assert_eq!(cfg.lb.tcp_pools.len(), 1);
    }

    #[test]
    fn invalid_toml_is_rejected() {
        let err = Config::load(Path::new("definitely-missing-file.toml"));
        assert!(err.is_err());
    }
}

// ---------------------------------------------------------------------------
// Tunnel lifecycle + config validation
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum TunnelState {
    Created,
    Connecting,
    Established,
    Down,
    Deleted,
}

#[derive(Clone, Debug)]
pub struct TunnelPeer {
    pub name: String,
    pub public_key: String,
    pub endpoint: Option<String>,
    pub allowed_ips: Vec<String>,
    pub state: TunnelState,
    pub created_at: u64,
}

pub fn validate_config(cfg: &Config) -> Vec<String> {
    let mut errors = Vec::new();
    for pool in &cfg.lb.tcp_pools {
        if pool.backends.is_empty() {
            errors.push(format!("tcp pool '{}': no backends", pool.name));
        }
    }
    for rule in &cfg.firewall {
        if rule.action != "allow" && rule.action != "deny" {
            errors.push(format!("firewall: invalid action '{}'", rule.action));
        }
    }
    if cfg.bgp.enabled && cfg.bgp.router_id.is_empty() {
        errors.push("bgp: router_id is empty".into());
    }
    errors
}

// ---------------------------------------------------------------------------
// Path Labels (flexiWAN-style) + Policy Engine (Viptela-style)
// ---------------------------------------------------------------------------

/// How a set of labeled paths is selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SelectionOrder {
    /// First healthy label wins; failover to the next.
    PriorityFailover,
    /// Spread traffic across healthy labeled paths.
    LoadBalance,
    /// Prefer paths meeting quality thresholds (loss/latency).
    QualityBased,
}

/// A logical label (ISP1, LTE, MPLS, ...) bound to WAN interfaces/tunnels.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct PathLabel {
    pub name: String,
    #[serde(default)]
    pub interfaces: Vec<String>,
    #[serde(default)]
    pub tunnels: Vec<String>,
    #[serde(default)]
    pub description: String,
}

/// L3/L4/app match for a policy rule.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
pub struct RuleMatch {
    /// Application category (e.g. "video", "voip") — free-form tag.
    #[serde(default)]
    pub app: Option<String>,
    #[serde(default)]
    pub protocol: Option<String>,
    #[serde(default)]
    pub src_prefix: Option<String>,
    #[serde(default)]
    pub dst_prefix: Option<String>,
    #[serde(default)]
    pub dst_port: Option<u16>,
}

impl RuleMatch {
    /// True if this match subsumes the other (used to validate ordering:
    /// a broader match must not shadow a narrower one before the default).
    pub fn is_match_all(&self) -> bool {
        self.app.is_none()
            && self.protocol.is_none()
            && self.src_prefix.is_none()
            && self.dst_prefix.is_none()
            && self.dst_port.is_none()
    }
}

/// Action: route matching traffic via labeled paths in the given order.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct RouteAction {
    pub labels: Vec<String>,
    pub order: SelectionOrder,
}

/// One rule inside a policy.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct PolicyRule {
    pub r#match: RuleMatch,
    pub action: RouteAction,
}

/// Ordered policy: rules evaluated top-down; implicit default match-all last.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Policy {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub rules: Vec<PolicyRule>,
    /// Final catch-all action when no rule matches.
    pub default_action: RouteAction,
    /// Installed on this device (Viptela-style per-device install).
    #[serde(default)]
    pub installed: bool,
}

impl Policy {
    /// Evaluate: first matching rule wins, else default action.
    /// True when `ip` falls inside the `cidr` network (same address family).
    pub fn cidr_contains(cidr: &str, ip: &str) -> bool {
        let Some((net, plen)) = cidr.split_once('/') else {
            return false;
        };
        let (Ok(net_ip), Ok(ip)) = (
            net.parse::<std::net::IpAddr>(),
            ip.parse::<std::net::IpAddr>(),
        ) else {
            return false;
        };
        if std::mem::discriminant(&net_ip) != std::mem::discriminant(&ip) {
            return false;
        }
        let Ok(plen) = plen.parse::<u32>() else {
            return false;
        };
        let max = if net_ip.is_ipv4() { 32 } else { 128 };
        if plen > max {
            return false;
        }
        match (net_ip, ip) {
            (std::net::IpAddr::V4(n), std::net::IpAddr::V4(i)) => {
                let mask = if plen == 0 {
                    0
                } else {
                    u32::MAX << (32 - plen)
                };
                (u32::from(n) & mask) == (u32::from(i) & mask)
            }
            (std::net::IpAddr::V6(n), std::net::IpAddr::V6(i)) => {
                let mask = if plen == 0 {
                    0u128
                } else {
                    u128::MAX << (128 - plen)
                };
                (u128::from(n) & mask) == (u128::from(i) & mask)
            }
            _ => false,
        }
    }

    pub fn evaluate(
        &self,
        app: Option<&str>,
        protocol: Option<&str>,
        src_prefix: Option<&str>,
        dst_prefix: Option<&str>,
        dst_port: Option<u16>,
    ) -> &RouteAction {
        for r in &self.rules {
            let m = &r.r#match;
            if let Some(a) = &m.app {
                if Some(a.as_str()) != app {
                    continue;
                }
            }
            if let Some(pr) = &m.protocol {
                if Some(pr.as_str()) != protocol {
                    continue;
                }
            }
            if let Some(sp) = &m.src_prefix {
                let hit = src_prefix.is_some_and(|traffic_ip| Self::cidr_contains(sp, traffic_ip));
                if !hit {
                    continue;
                }
            }
            if let Some(dp) = &m.dst_prefix {
                let hit = dst_prefix.is_some_and(|traffic_ip| Self::cidr_contains(dp, traffic_ip));
                if !hit {
                    continue;
                }
            }
            if let Some(p) = m.dst_port {
                if Some(p) != dst_port {
                    continue;
                }
            }
            return &r.action;
        }
        &self.default_action
    }

    /// Validate: only the implicit default may be match-all; explicit rules
    /// must narrow the match.
    pub fn validate(&self) -> Result<(), String> {
        for (i, r) in self.rules.iter().enumerate() {
            if r.r#match.is_match_all() {
                return Err(format!(
                    "rule {} is match-all; only the implicit default may match all",
                    i
                ));
            }
            if r.action.labels.is_empty() {
                return Err(format!("rule {} routes via no labels", i));
            }
        }
        if self.default_action.labels.is_empty() {
            return Err("default action routes via no labels".into());
        }
        Ok(())
    }
}

/// Persisted path-label + policy store (atomic JSON file, 0600).
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
pub struct PathPolicyStore {
    #[serde(default)]
    pub labels: Vec<PathLabel>,
    #[serde(default)]
    pub policies: Vec<Policy>,
}

#[cfg(test)]
mod path_policy_tests {
    use super::*;

    fn action(labels: &[&str], order: SelectionOrder) -> RouteAction {
        RouteAction {
            labels: labels.iter().map(|s| s.to_string()).collect(),
            order,
        }
    }

    fn sample_policy() -> Policy {
        Policy {
            name: "voip-priority".into(),
            description: String::new(),
            rules: vec![PolicyRule {
                r#match: RuleMatch {
                    app: Some("voip".into()),
                    ..Default::default()
                },
                action: action(&["MPLS"], SelectionOrder::PriorityFailover),
            }],
            default_action: action(&["ISP1", "LTE"], SelectionOrder::LoadBalance),
            installed: false,
        }
    }

    #[test]
    fn evaluate_matches_rule_then_default() {
        let p = sample_policy();
        assert_eq!(
            p.evaluate(Some("voip"), None, None, None, None).labels,
            vec!["MPLS".to_string()]
        );
        assert_eq!(
            p.evaluate(Some("video"), None, None, None, None).labels,
            vec!["ISP1".to_string(), "LTE".to_string()]
        );
        assert_eq!(p.evaluate(None, None, None, None, None), &p.default_action);
    }

    #[test]
    fn validate_rejects_match_all_rule_and_empty_labels() {
        let mut p = sample_policy();
        assert!(p.validate().is_ok());
        p.rules.insert(
            0,
            PolicyRule {
                r#match: RuleMatch::default(),
                action: action(&["X"], SelectionOrder::PriorityFailover),
            },
        );
        assert!(p.validate().is_err());
        p.rules.remove(0);
        p.default_action.labels.clear();
        assert!(p.validate().is_err());
    }

    #[test]
    fn evaluate_matches_src_dst_prefix() {
        let mut p = sample_policy();
        p.rules.push(PolicyRule {
            r#match: RuleMatch {
                src_prefix: Some("10.0.0.0/8".into()),
                dst_prefix: Some("192.168.1.0/24".into()),
                ..Default::default()
            },
            action: action(&["LTE"], SelectionOrder::PriorityFailover),
        });
        p.rules.push(PolicyRule {
            r#match: RuleMatch {
                protocol: Some("udp".into()),
                dst_port: Some(53),
                ..Default::default()
            },
            action: action(&["ISP1"], SelectionOrder::LoadBalance),
        });
        assert_eq!(
            p.evaluate(None, None, Some("10.1.2.3"), Some("192.168.1.50"), None)
                .labels,
            vec!["LTE".to_string()]
        );
        assert_eq!(
            p.evaluate(None, Some("udp"), None, None, Some(53)).labels,
            vec!["ISP1".to_string()]
        );
        // src matches but dst does not -> falls through to default
        assert_eq!(
            p.evaluate(None, None, Some("10.1.2.3"), Some("10.9.9.9"), None),
            &p.default_action
        );
    }

    #[test]
    fn selection_order_serializes_kebab() {
        assert_eq!(
            serde_json::to_string(&SelectionOrder::PriorityFailover).unwrap(),
            r#""priority-failover""#
        );
        assert_eq!(
            serde_json::to_string(&SelectionOrder::QualityBased).unwrap(),
            r#""quality-based""#
        );
    }

    #[test]
    fn store_roundtrips() {
        let mut st = PathPolicyStore::default();
        st.labels.push(PathLabel {
            name: "ISP1".into(),
            interfaces: vec!["wan0".into()],
            tunnels: vec![],
            description: "primary fiber".into(),
        });
        st.policies.push(sample_policy());
        let json = serde_json::to_string(&st).unwrap();
        let back: PathPolicyStore = serde_json::from_str(&json).unwrap();
        assert_eq!(back, st);
    }
}
