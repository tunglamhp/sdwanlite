//! REST API + embedded web dashboard for sdwanlite.

use sdwanlite_bgp::{BgpSpeaker, SessionState};
use sdwanlite_lb::tcp::TcpLoadBalancer;
use sdwanlite_lb::HttpLoadBalancer;
use serde::Serialize;
use std::sync::Arc;
use std::time::Instant;

pub struct AppState {
    pub config: Arc<sdwanlite_core::Config>,
    pub started: Instant,
    pub tcp_pools: Vec<Arc<TcpLoadBalancer>>,
    pub http_pools: Vec<Arc<HttpLoadBalancer>>,
    pub bgp: Option<Arc<BgpSpeaker>>,
}

#[derive(Serialize)]
struct BackendView {
    addr: String,
    healthy: bool,
    active_conns: u64,
    total_conns: u64,
}

#[derive(Serialize)]
struct TcpPoolView {
    name: String,
    algorithm: &'static str,
    backends: Vec<BackendView>,
}

#[derive(Serialize)]
struct HttpRouteView {
    host: String,
    path_prefix: String,
    backends: usize,
}

#[derive(Serialize)]
struct HttpPoolView {
    name: String,
    routes: Vec<HttpRouteView>,
}

#[derive(Serialize)]
struct StatusView {
    node: String,
    version: &'static str,
    uptime_secs: u64,
    mesh_enabled: bool,
    mesh_peers_configured: usize,
    bgp_enabled: bool,
    bgp_sessions: Vec<SessionView>,
    bgp_rib_size: usize,
    lb: LbSummaryView,
}

#[derive(Serialize)]
struct SessionView {
    neighbor: String,
    state: String,
}

#[derive(Serialize)]
struct LbSummaryView {
    tcp_pools: usize,
    http_pools: usize,
}

pub fn router(state: Arc<AppState>) -> axum::Router {
    use axum::routing::get;
    axum::Router::new()
        .route("/", get(dashboard))
        .route("/api/status", get(api_status))
        .route("/api/lb", get(api_lb))
        .route("/api/mesh/keypair", get(api_keypair))
        .route("/api/bgp/rib", get(api_rib))
        .with_state(state)
}

async fn dashboard() -> axum::response::Html<&'static str> {
    axum::response::Html(include_str!("dashboard.html"))
}

async fn api_status(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
) -> axum::Json<StatusView> {
    let mut sessions = Vec::new();
    let mut rib_size = 0;
    if let Some(bgp) = &state.bgp {
        for (n, s) in bgp.sessions.read().await.iter() {
            sessions.push(SessionView { neighbor: n.clone(), state: s.as_str().to_string() });
        }
        rib_size = bgp.rib.read().await.len();
    }

    axum::Json(StatusView {
        node: state.config.general.name.clone(),
        version: env!("CARGO_PKG_VERSION"),
        uptime_secs: state.started.elapsed().as_secs(),
        mesh_enabled: state.config.mesh.enabled,
        mesh_peers_configured: state.config.mesh.peers.len(),
        bgp_enabled: state.config.bgp.enabled,
        bgp_sessions: sessions,
        bgp_rib_size: rib_size,
        lb: LbSummaryView {
            tcp_pools: state.tcp_pools.len(),
            http_pools: state.http_pools.len(),
        },
    })
}

async fn api_lb(axum::extract::State(state): axum::extract::State<Arc<AppState>>) -> axum::Json<serde_json::Value> {
    let mut tcp = Vec::new();
    for pool in &state.tcp_pools {
        tcp.push(TcpPoolView {
            name: pool.name.clone(),
            algorithm: pool.algorithm().as_str(),
            backends: pool
                .backends()
                .iter()
                .map(|b| BackendView {
                    addr: b.addr.to_string(),
                    healthy: b.is_healthy(),
                    active_conns: b.active_conns(),
                    total_conns: b.total_conns(),
                })
                .collect(),
        });
    }
    let mut http = Vec::new();
    for pool in &state.http_pools {
        http.push(HttpPoolView {
            name: pool.name.clone(),
            routes: pool
                .route_info()
                .into_iter()
                .map(|(host, path_prefix, backends)| HttpRouteView {
                    host,
                    path_prefix,
                    backends,
                })
                .collect(),
        });
    }
    axum::Json(serde_json::json!({ "tcp": tcp, "http": http }))
}

async fn api_keypair() -> axum::Json<serde_json::Value> {
    let kp = sdwanlite_mesh::generate_keypair();
    axum::Json(serde_json::json!({
        "private_key": kp.private_b64,
        "public_key": kp.public_b64
    }))
}

async fn api_rib(axum::extract::State(state): axum::extract::State<Arc<AppState>>) -> axum::Json<serde_json::Value> {
    match &state.bgp {
        Some(bgp) => {
            let rib = bgp.rib.read().await;
            let routes: Vec<serde_json::Value> = rib
                .iter()
                .map(|(n, p)| serde_json::json!({ "neighbor": n, "prefix": p.to_string() }))
                .collect();
            axum::Json(serde_json::json!({ "count": routes.len(), "routes": routes }))
        }
        None => axum::Json(serde_json::json!({ "count": 0, "routes": [] })),
    }
}

// silence unused import when BGP feature compiled out in the future
#[allow(dead_code)]
fn _assert_state(_: SessionState) {}
