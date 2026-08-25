//! REST API client + data types for the control panel.

use gloo_net::http::{Method, Request};
use serde::Deserialize;

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Status {
    pub node: String,
    pub version: String,
    pub uptime_secs: u64,
    pub mesh_enabled: bool,
    pub mesh_peers_configured: usize,
    pub bgp_enabled: bool,
    #[serde(default)]
    pub bgp_sessions: Vec<BgpSession>,
    #[serde(default)]
    pub bgp_rib_size: usize,
    #[serde(default)]
    pub lb: LbCounts,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct BgpSession {
    pub neighbor: String,
    pub state: String,
    #[serde(default)]
    pub remote_as: Option<u32>,
    #[serde(default)]
    pub negotiated_hold_secs: u16,
    #[serde(default)]
    pub prefixes_received: u64,
    #[serde(default)]
    pub updates_received: u64,
    #[serde(default)]
    pub flaps: u64,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct LbCounts {
    #[serde(default)]
    pub tcp_pools: usize,
    #[serde(default)]
    pub http_pools: usize,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct LbData {
    #[serde(default)]
    pub tcp: Vec<TcpPoolView>,
    #[serde(default)]
    pub http: Vec<HttpPoolView>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct TcpPoolView {
    pub name: String,
    pub algorithm: String,
    #[serde(default)]
    pub active_conns: u64,
    #[serde(default)]
    pub rejected_conns: u64,
    #[serde(default)]
    pub backends: Vec<BackendView>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct BackendView {
    pub addr: String,
    pub healthy: bool,
    #[serde(default)]
    pub active_conns: u64,
    #[serde(default)]
    pub total_conns: u64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct HttpPoolView {
    pub name: String,
    #[serde(default)]
    pub routes: Vec<HttpRouteView>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct HttpRouteView {
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub path_prefix: String,
    #[serde(default)]
    pub backends: usize,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct MeshStatus {
    pub available: bool,
    #[serde(default)]
    pub peers: Vec<WgPeerView>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct WgPeerView {
    pub public_key: String,
    pub endpoint: Option<String>,
    #[serde(default)]
    pub allowed_ips: Vec<String>,
    pub latest_handshake_secs_ago: Option<u64>,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct RibData {
    pub count: usize,
    #[serde(default)]
    pub routes: Vec<RibRoute>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct RibRoute {
    pub prefix: String,
    pub neighbor: String,
    #[serde(default)]
    pub as_path_len: Option<u64>,
    #[serde(default)]
    pub best: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Keypair {
    pub private_key: String,
    pub public_key: String,
}

async fn get_json<T: for<'de> Deserialize<'de>>(url: &str) -> Result<T, String> {
    let rsp = Request::get(url).send().await.map_err(|e| e.to_string())?;
    if !rsp.ok() {
        return Err(format!("HTTP {}", rsp.status()));
    }
    rsp.json::<T>().await.map_err(|e| e.to_string())
}

pub fn api_status() -> impl Future<Output = Result<Status, String>> {
    get_json("/api/status")
}

pub fn api_lb() -> impl Future<Output = Result<LbData, String>> {
    get_json("/api/lb")
}

pub fn api_mesh_status() -> impl Future<Output = Result<MeshStatus, String>> {
    get_json("/api/mesh/status")
}

pub fn api_rib() -> impl Future<Output = Result<RibData, String>> {
    get_json("/api/bgp/rib")
}

pub fn api_keypair() -> impl Future<Output = Result<Keypair, String>> {
    get_json("/api/mesh/keypair")
}

pub async fn api_call(method: &str, url: &str) -> Result<String, String> {
    let m = match method {
        "POST" => Method::POST,
        "DELETE" => Method::DELETE,
        _ => Method::GET,
    };
    let rsp = Request::new(url)
        .method(m)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let text = rsp.text().await.map_err(|e| e.to_string())?;
    Ok(format!("HTTP {} — {}", rsp.status(), text))
}

// std Future re-export so the signatures above stay short
pub use std::future::Future;
