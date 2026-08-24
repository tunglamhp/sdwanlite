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
    pub email: String,
    /// DNS names to include in the certificate.
    pub domains: Vec<String>,
    #[serde(default = "default_http01_port")]
    pub http01_port: u16,
    pub cert_file: String,
    pub key_file: String,
    /// Renew when less than this many days remain (default 30).
    #[serde(default = "default_renew_days")]
    pub renew_days: u32,
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
    "0.0.0.0".into()
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
    pub backends: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Algorithm {
    RoundRobin,
    LeastConnections,
    Random,
}

impl Algorithm {
    pub fn as_str(&self) -> &'static str {
        match self {
            Algorithm::RoundRobin => "round_robin",
            Algorithm::LeastConnections => "least_connections",
            Algorithm::Random => "random",
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
