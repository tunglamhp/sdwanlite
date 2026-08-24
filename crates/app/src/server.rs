//! REST API + embedded web dashboard for sdwanlite.

use sdwanlite_bgp::BgpSpeaker;
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
    active_conns: usize,
    rejected_conns: u64,
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
    remote_as: Option<u32>,
    negotiated_hold_secs: u16,
    prefixes_received: u64,
    updates_received: u64,
    flaps: u64,
}

#[derive(Serialize)]
struct LbSummaryView {
    tcp_pools: usize,
    http_pools: usize,
}

pub fn router(state: Arc<AppState>) -> axum::Router {
    use axum::routing::{get, post};
    axum::Router::new()
        .route("/", get(dashboard))
        .route("/api/status", get(api_status))
        .route("/api/lb", get(api_lb))
        .route("/api/mesh/keypair", get(api_keypair))
        .route("/api/mesh/status", get(api_mesh_status))
        .route("/api/bgp/rib", get(api_rib))
        .route(
            "/api/lb/tcp/:name/backends",
            post(api_add_backend).delete(api_remove_backend),
        )
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
        for (n, i) in bgp.sessions.read().await.iter() {
            sessions.push(SessionView {
                neighbor: n.clone(),
                state: i.state.as_str().to_string(),
                remote_as: i.remote_as,
                negotiated_hold_secs: i.negotiated_hold_secs,
                prefixes_received: i.prefixes_received,
                updates_received: i.updates_received,
                flaps: i.flaps,
            });
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
            active_conns: pool.active_conns(),
            rejected_conns: pool.rejected_conns(),
            backends: pool
                .backends()
                .await
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

#[derive(Serialize)]
struct MeshPeerView {
    public_key: String,
    endpoint: Option<String>,
    allowed_ips: Vec<String>,
    latest_handshake_secs_ago: Option<u64>,
    rx_bytes: u64,
    tx_bytes: u64,
}

async fn api_mesh_status() -> axum::Json<serde_json::Value> {
    match sdwanlite_mesh::status().await {
        Ok(peers) => {
            let views: Vec<MeshPeerView> = peers
                .into_iter()
                .map(|p| MeshPeerView {
                    public_key: p.public_key,
                    endpoint: p.endpoint,
                    allowed_ips: p.allowed_ips,
                    latest_handshake_secs_ago: p.latest_handshake_secs_ago,
                    rx_bytes: p.rx_bytes,
                    tx_bytes: p.tx_bytes,
                })
                .collect();
            axum::Json(serde_json::json!({ "available": true, "peers": views }))
        }
        Err(e) => axum::Json(serde_json::json!({ "available": false, "error": e.to_string() })),
    }
}

fn authorized(state: &AppState, headers: &axum::http::HeaderMap) -> bool {
    match &state.config.general.api_token {
        None => true, // no token configured -> mutations open (lab default)
        Some(t) => headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .map(|v| v == format!("Bearer {t}"))
            .unwrap_or(false),
    }
}

async fn api_add_backend(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    axum::extract::Path(name): axum::extract::Path<String>,
    headers: axum::http::HeaderMap,
    axum::Json(body): axum::Json<serde_json::Value>,
) -> axum::Json<serde_json::Value> {
    if !authorized(&state, &headers) {
        return axum::Json(serde_json::json!({ "ok": false, "error": "unauthorized" }));
    }
    let Some(pool) = state.tcp_pools.iter().find(|p| p.name == name) else {
        return axum::Json(serde_json::json!({ "ok": false, "error": "pool not found" }));
    };
    let Some(addr) = body.get("addr").and_then(|a| a.as_str()).and_then(|a| a.parse().ok()) else {
        return axum::Json(serde_json::json!({ "ok": false, "error": "invalid addr" }));
    };
    let added = pool.add_backend(addr).await;
    axum::Json(serde_json::json!({ "ok": added }))
}

async fn api_remove_backend(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    axum::extract::Path(name): axum::extract::Path<String>,
    headers: axum::http::HeaderMap,
    axum::Json(body): axum::Json<serde_json::Value>,
) -> axum::Json<serde_json::Value> {
    if !authorized(&state, &headers) {
        return axum::Json(serde_json::json!({ "ok": false, "error": "unauthorized" }));
    }
    let Some(pool) = state.tcp_pools.iter().find(|p| p.name == name) else {
        return axum::Json(serde_json::json!({ "ok": false, "error": "pool not found" }));
    };
    let Some(addr) = body.get("addr").and_then(|a| a.as_str()).and_then(|a| a.parse().ok()) else {
        return axum::Json(serde_json::json!({ "ok": false, "error": "invalid addr" }));
    };
    let removed = pool.remove_backend(addr).await;
    axum::Json(serde_json::json!({ "ok": removed }))
}
