//! REST API + embedded web dashboard for sdwanlite.

use axum::response::IntoResponse;
use futures_util::StreamExt as _;
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
    pub alerts: Arc<sdwanlite_lb::AlertLog>,
    /// Path labels + policies, persisted atomically to `path-policy.json`.
    pub path_policy: std::sync::Mutex<sdwanlite_core::PathPolicyStore>,
    pub path_policy_path: std::path::PathBuf,
    /// Per-pool runtime overrides (algorithm, max_conns, drain, hc params).
    pub pool_overrides: std::sync::Mutex<std::collections::HashMap<String, PoolConfigOverride>>,
    pub pool_overrides_path: std::path::PathBuf,
}

/// Persisted per-pool overrides. `hc_*` apply on restart; the rest apply live.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct PoolConfigOverride {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub algorithm: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_conns: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drain: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hc_interval_secs: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hc_timeout_secs: Option<u64>,
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
    auth_enabled: bool,
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

/// HTTP Basic Auth middleware — reads credentials from env vars.
/// If SDWANLITE_AUTH_USER + SDWANLITE_AUTH_PASS are set, ALL routes require auth.
/// If not set, access is open (dev mode) but a warning is logged at startup.
pub async fn auth_middleware(
    req: axum::http::Request<axum::body::Body>,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let user = std::env::var("SDWANLITE_AUTH_USER").unwrap_or_default();
    let pass = std::env::var("SDWANLITE_AUTH_PASS").unwrap_or_default();

    // Skip auth in dev mode (no credentials configured)
    if user.is_empty() || pass.is_empty() {
        return next.run(req).await;
    }

    let expected = format!("Basic {}", {
        use base64::Engine as _;
        base64::engine::general_purpose::STANDARD.encode(format!("{}:{}", user, pass))
    });

    let authorized = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .map(|v| ct_eq(v.as_bytes(), expected.as_bytes()))
        .unwrap_or(false);
    if authorized {
        next.run(req).await
    } else {
        let mut rsp = axum::response::Response::new(axum::body::Body::from("Unauthorized"));
        *rsp.status_mut() = axum::http::StatusCode::UNAUTHORIZED;
        rsp.headers_mut().insert(
            axum::http::header::WWW_AUTHENTICATE,
            axum::http::HeaderValue::from_static("Basic realm=\"sdwanlite\""),
        );
        rsp
    }
}

pub fn router(state: Arc<AppState>) -> axum::Router {
    use axum::routing::{get, post, put};
    axum::Router::new()
        .route("/api/status", get(api_status))
        .route("/api/lb", get(api_lb))
        .route("/api/signals", get(api_signals))
        .route("/api/labels", get(api_labels).put(api_labels_put))
        .route("/api/policies", get(api_policies).put(api_policies_put))
        .route("/api/policies/:name/install", post(api_policy_install))
        .route("/api/policies/:name/uninstall", post(api_policy_uninstall))
        .route("/api/mesh/keypair", get(api_keypair))
        .route("/api/mesh/status", get(api_mesh_status))
        .route("/metrics", get(api_metrics))
        .route("/api/events", get(api_events))
        .route("/api/reload", post(api_reload))
        .route("/api/tls/reload", post(api_tls_reload))
        .route("/api/tls/status", get(api_tls_status))
        .route("/api/acme/issue", post(api_acme_issue))
        .route("/api/alerts", get(api_alerts))
        .route(
            "/api/firewall",
            get(api_firewall_list)
                .post(api_firewall_add)
                .delete(api_firewall_delete),
        )
        .route("/api/validate", post(api_validate))
        .route("/api/bgp/rib", get(api_rib))
        .route(
            "/api/lb/tcp/:name/config",
            put(api_pool_config_put).get(api_pool_config_get),
        )
        .route(
            "/api/lb/tcp/:name/backends",
            post(api_add_backend).delete(api_remove_backend),
        )
        .with_state(state)
}

pub async fn legacy_dashboard() -> impl axum::response::IntoResponse {
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

    let auth_enabled = std::env::var("SDWANLITE_AUTH_USER")
        .map(|v| !v.is_empty())
        .unwrap_or(false);
    axum::Json(StatusView {
        node: state.config.general.name.clone(),
        version: env!("CARGO_PKG_VERSION"),
        uptime_secs: state.started.elapsed().as_secs(),
        mesh_enabled: state.config.mesh.enabled,
        mesh_peers_configured: state.config.mesh.peers.len(),
        bgp_enabled: state.config.bgp.enabled,
        bgp_sessions: sessions,
        bgp_rib_size: rib_size,
        auth_enabled,
        lb: LbSummaryView {
            tcp_pools: state.tcp_pools.len(),
            http_pools: state.http_pools.len(),
        },
    })
}

#[derive(Serialize)]
struct SignalBackendView {
    addr: String,
    healthy: bool,
    latency_p50_us: Option<u64>,
    latency_p95_us: Option<u64>,
    rx_bytes: u64,
    tx_bytes: u64,
    active_conns: u64,
    health_failures: u64,
}

#[derive(Serialize)]
struct SignalPoolView {
    name: String,
    latency_p50_us: Option<u64>,
    latency_p95_us: Option<u64>,
    rx_bytes: u64,
    tx_bytes: u64,
    errors: SignalErrorsView,
    saturation: SignalSaturationView,
    backends: Vec<SignalBackendView>,
}

#[derive(Serialize)]
struct SignalErrorsView {
    unhealthy_backends: usize,
    total_backends: usize,
    loss_pct: f64,
    health_failures: u64,
    rejected_conns: u64,
}

#[derive(Serialize)]
struct SignalSaturationView {
    active_conns: usize,
    max_conns: usize, // 0 = unlimited
    utilization_pct: f64,
}

#[derive(Serialize)]
struct SignalsView {
    pools: Vec<SignalPoolView>,
    totals: SignalTotalsView,
}

#[derive(Serialize)]
struct SignalTotalsView {
    latency_p50_us: Option<u64>,
    latency_p95_us: Option<u64>,
    rx_bytes: u64,
    tx_bytes: u64,
    unhealthy_backends: usize,
    total_backends: usize,
    health_failures: u64,
    rejected_conns: u64,
    active_conns: usize,
    max_conns: usize,
    utilization_pct: f64,
}

/// Google SRE Four Golden Signals per pool and system-wide.
async fn api_signals(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
) -> axum::Json<SignalsView> {
    let mut pools = Vec::new();
    let mut tot_rx = 0u64;
    let mut tot_tx = 0u64;
    let mut tot_unhealthy = 0usize;
    let mut tot_backends = 0usize;
    let mut tot_hc_fail = 0u64;
    let mut tot_rejected = 0u64;
    let mut tot_active = 0usize;
    let mut tot_max = 0usize;
    let mut all_lat: Vec<u64> = Vec::new();

    for pool in &state.tcp_pools {
        let backends = pool.backends().await;
        let total = backends.len();
        let unhealthy = backends.iter().filter(|b| !b.is_healthy()).count();
        let mut pool_rx = 0u64;
        let mut pool_tx = 0u64;
        let mut pool_hc = 0u64;
        let mut bv = Vec::with_capacity(total);
        for b in &backends {
            let (p50, p95) = b.latency_percentiles_us();
            if let Some(x) = p50 {
                all_lat.push(x);
            }
            pool_rx += b.rx_bytes();
            pool_tx += b.tx_bytes();
            pool_hc += b.health_failures();
            bv.push(SignalBackendView {
                addr: b.addr.to_string(),
                healthy: b.is_healthy(),
                latency_p50_us: p50,
                latency_p95_us: p95,
                rx_bytes: b.rx_bytes(),
                tx_bytes: b.tx_bytes(),
                active_conns: b.active_conns(),
                health_failures: b.health_failures(),
            });
        }
        let active = pool.active_conns();
        let max = pool.max_conns();
        let loss = if total > 0 {
            unhealthy as f64 * 100.0 / total as f64
        } else {
            0.0
        };
        let util = if max > 0 {
            active as f64 * 100.0 / max as f64
        } else {
            0.0
        };
        tot_rx += pool_rx;
        tot_tx += pool_tx;
        tot_unhealthy += unhealthy;
        tot_backends += total;
        tot_hc_fail += pool_hc;
        tot_rejected += pool.rejected_conns();
        tot_active += active;
        tot_max += max;
        pools.push(SignalPoolView {
            name: pool.name.clone(),
            latency_p50_us: percentile(&all_lat, 50),
            latency_p95_us: percentile(&all_lat, 95),
            rx_bytes: pool_rx,
            tx_bytes: pool_tx,
            errors: SignalErrorsView {
                unhealthy_backends: unhealthy,
                total_backends: total,
                loss_pct: (loss * 100.0).round() / 100.0,
                health_failures: pool_hc,
                rejected_conns: pool.rejected_conns(),
            },
            saturation: SignalSaturationView {
                active_conns: active,
                max_conns: max,
                utilization_pct: (util * 100.0).round() / 100.0,
            },
            backends: bv,
        });
    }

    for pool in &state.http_pools {
        let routes = pool.backends_by_route();
        let mut seen: std::collections::HashSet<String> = Default::default();
        let mut pool_rx = 0u64;
        let mut pool_tx = 0u64;
        let mut pool_hc = 0u64;
        let mut bv = Vec::new();
        let mut unhealthy = 0usize;
        let mut total = 0usize;
        for backends in &routes {
            for b in backends {
                let key = b.addr.to_string();
                if !seen.insert(key.clone()) {
                    continue; // same backend may serve multiple routes
                }
                total += 1;
                if !b.is_healthy() {
                    unhealthy += 1;
                }
                let (p50, p95) = b.latency_percentiles_us();
                if let Some(x) = p50 {
                    all_lat.push(x);
                }
                pool_rx += b.rx_bytes();
                pool_tx += b.tx_bytes();
                pool_hc += b.health_failures();
                bv.push(SignalBackendView {
                    addr: key,
                    healthy: b.is_healthy(),
                    latency_p50_us: p50,
                    latency_p95_us: p95,
                    rx_bytes: b.rx_bytes(),
                    tx_bytes: b.tx_bytes(),
                    active_conns: b.active_conns(),
                    health_failures: b.health_failures(),
                });
            }
        }
        let active = pool.active_conns();
        let max = 0usize; // http pools: unlimited by default
        let loss = if total > 0 {
            unhealthy as f64 * 100.0 / total as f64
        } else {
            0.0
        };
        tot_rx += pool_rx;
        tot_tx += pool_tx;
        tot_unhealthy += unhealthy;
        tot_backends += total;
        tot_hc_fail += pool_hc;
        tot_rejected += pool.rejected_conns();
        tot_active += active;
        pools.push(SignalPoolView {
            name: pool.name.clone(),
            latency_p50_us: percentile(&all_lat, 50),
            latency_p95_us: percentile(&all_lat, 95),
            rx_bytes: pool_rx,
            tx_bytes: pool_tx,
            errors: SignalErrorsView {
                unhealthy_backends: unhealthy,
                total_backends: total,
                loss_pct: (loss * 100.0).round() / 100.0,
                health_failures: pool_hc,
                rejected_conns: pool.rejected_conns(),
            },
            saturation: SignalSaturationView {
                active_conns: active,
                max_conns: max,
                utilization_pct: 0.0,
            },
            backends: bv,
        });
    }

    let util = if tot_max > 0 {
        tot_active as f64 * 100.0 / tot_max as f64
    } else {
        0.0
    };
    axum::Json(SignalsView {
        totals: SignalTotalsView {
            latency_p50_us: percentile(&all_lat, 50),
            latency_p95_us: percentile(&all_lat, 95),
            rx_bytes: tot_rx,
            tx_bytes: tot_tx,
            unhealthy_backends: tot_unhealthy,
            total_backends: tot_backends,
            health_failures: tot_hc_fail,
            rejected_conns: tot_rejected,
            active_conns: tot_active,
            max_conns: tot_max,
            utilization_pct: (util * 100.0).round() / 100.0,
        },
        pools,
    })
}

/// Nearest-rank percentile of a sampled list (ignores zeros).
fn percentile(samples: &[u64], q: usize) -> Option<u64> {
    let mut v: Vec<u64> = samples.iter().copied().filter(|x| *x > 0).collect();
    if v.is_empty() {
        return None;
    }
    v.sort_unstable();
    Some(v[(v.len() * q).div_ceil(100).max(1) - 1])
}

// ---------------------------------------------------------------------------
// Path Labels + Policy Engine API
// ---------------------------------------------------------------------------

fn save_path_policy(state: &AppState) -> Result<(), String> {
    use std::io::Write;
    let store = state.path_policy.lock().map_err(|_| "store poisoned")?;
    let tmp = state.path_policy_path.with_extension("tmp");
    let json = serde_json::to_vec_pretty(&*store).map_err(|e| e.to_string())?;
    // Create the temp file with restrictive permissions up front so secrets
    // are never briefly world-readable, and fsync before the atomic rename.
    #[cfg(unix)]
    let mut f = {
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)
            .map_err(|e| e.to_string())?
    };
    #[cfg(not(unix))]
    let mut f = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
    f.write_all(&json).map_err(|e| e.to_string())?;
    f.sync_all().map_err(|e| e.to_string())?;
    drop(f);
    std::fs::rename(&tmp, &state.path_policy_path).map_err(|e| e.to_string())
}

pub fn lock_overrides(
    state: &AppState,
) -> std::sync::MutexGuard<'_, std::collections::HashMap<String, PoolConfigOverride>> {
    match state.pool_overrides.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn lock_store(state: &AppState) -> std::sync::MutexGuard<'_, sdwanlite_core::PathPolicyStore> {
    // Recover data even from a poisoned lock: the store is plain JSON state.
    match state.path_policy.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    }
}

pub fn load_path_policy(path: &std::path::Path) -> sdwanlite_core::PathPolicyStore {
    std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

async fn api_labels(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
) -> axum::Json<serde_json::Value> {
    let store = lock_store(&state);
    axum::Json(serde_json::json!({ "labels": store.labels }))
}

async fn api_labels_put(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    axum::Json(labels): axum::Json<Vec<sdwanlite_core::PathLabel>>,
) -> axum::response::Response {
    let mut seen = std::collections::HashSet::new();
    for l in &labels {
        if l.name.trim().is_empty() {
            return err_json(400, "label name must not be empty");
        }
        if !seen.insert(l.name.trim().to_string()) {
            return err_json(400, &format!("duplicate label name: {}", l.name));
        }
    }
    let mut store = lock_store(&state);
    store.labels = labels;
    drop(store);
    match save_path_policy(&state) {
        Ok(()) => axum::Json(serde_json::json!({ "ok": true })).into_response(),
        Err(e) => err_json(500, &e),
    }
}

async fn api_policies(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
) -> axum::Json<serde_json::Value> {
    let store = lock_store(&state);
    axum::Json(serde_json::json!({ "policies": store.policies }))
}

async fn api_policies_put(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    axum::Json(policies): axum::Json<Vec<sdwanlite_core::Policy>>,
) -> axum::response::Response {
    for p in &policies {
        if let Err(e) = p.validate() {
            return err_json(400, &format!("policy {}: {}", p.name, e));
        }
    }
    let mut store = lock_store(&state);
    for p in &policies {
        for a in p
            .rules
            .iter()
            .map(|r| &r.action)
            .chain(std::iter::once(&p.default_action))
        {
            for lbl in &a.labels {
                if !store.labels.iter().any(|l| &l.name == lbl) {
                    return err_json(
                        400,
                        &format!("policy {}: references undefined label {lbl}", p.name),
                    );
                }
            }
        }
    }
    store.policies = policies;
    drop(store);
    match save_path_policy(&state) {
        Ok(()) => axum::Json(serde_json::json!({ "ok": true })).into_response(),
        Err(e) => err_json(500, &e),
    }
}

async fn api_policy_install(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    axum::extract::Path(name): axum::extract::Path<String>,
) -> axum::response::Response {
    set_policy_installed(&state, &name, true)
}

async fn api_policy_uninstall(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    axum::extract::Path(name): axum::extract::Path<String>,
) -> axum::response::Response {
    set_policy_installed(&state, &name, false)
}

fn set_policy_installed(state: &AppState, name: &str, installed: bool) -> axum::response::Response {
    let mut store = match state.path_policy.lock() {
        Ok(s) => s,
        Err(_) => return err_json(500, "store poisoned"),
    };
    match store.policies.iter_mut().find(|p| p.name == name) {
        Some(p) => p.installed = installed,
        None => return err_json(404, &format!("policy {name} not found")),
    }
    drop(store);
    match save_path_policy(state) {
        Ok(()) => {
            axum::Json(serde_json::json!({ "ok": true, "installed": installed })).into_response()
        }
        Err(e) => err_json(500, &e),
    }
}

fn err_json(code: u16, msg: &str) -> axum::response::Response {
    let mut rsp = axum::response::Response::new(axum::body::Body::from(msg.to_string()));
    *rsp.status_mut() = axum::http::StatusCode::from_u16(code)
        .unwrap_or(axum::http::StatusCode::INTERNAL_SERVER_ERROR);
    rsp.headers_mut().insert(
        axum::http::header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("application/json"),
    );
    rsp
}

// ---------------------------------------------------------------------------
// Pool config overrides API
// ---------------------------------------------------------------------------

pub fn load_pool_overrides(
    path: &std::path::Path,
) -> std::collections::HashMap<String, PoolConfigOverride> {
    std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn save_pool_overrides(state: &AppState) -> Result<(), String> {
    let map = state.pool_overrides.lock().map_err(|_| "poisoned")?;
    let tmp = state.pool_overrides_path.with_extension("tmp");
    let json = serde_json::to_vec_pretty(&*map).map_err(|e| e.to_string())?;
    std::fs::write(&tmp, &json).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
    }
    std::fs::rename(&tmp, &state.pool_overrides_path).map_err(|e| e.to_string())
}

/// Apply a stored override to a live TCP pool (best effort).
/// Parse algorithm name (snake_case, matching config serde names).
pub fn parse_algorithm(s: &str) -> Option<sdwanlite_core::Algorithm> {
    match s {
        "round_robin" | "round-robin" => Some(sdwanlite_core::Algorithm::RoundRobin),
        "least_connections" | "least-connections" => {
            Some(sdwanlite_core::Algorithm::LeastConnections)
        }
        "random" => Some(sdwanlite_core::Algorithm::Random),
        "failover" => Some(sdwanlite_core::Algorithm::Failover),
        _ => None,
    }
}

pub fn apply_override_live(pool: &sdwanlite_lb::TcpLoadBalancer, o: &PoolConfigOverride) {
    if let Some(a) = &o.algorithm {
        if let Some(algo) = parse_algorithm(a) {
            pool.set_algorithm(algo);
        }
    }
    if let Some(m) = o.max_conns {
        pool.set_max_conns(m);
    }
    if let Some(d) = o.drain {
        pool.set_drained(d);
    }
}

async fn api_pool_config_get(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    axum::extract::Path(name): axum::extract::Path<String>,
) -> axum::response::Response {
    let map = lock_overrides(&state);
    match map.get(&name) {
        Some(o) => axum::Json(o).into_response(),
        None => err_json(404, &format!("no overrides for pool {name}")),
    }
}

async fn api_pool_config_put(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    axum::extract::Path(name): axum::extract::Path<String>,
    axum::Json(o): axum::Json<PoolConfigOverride>,
) -> axum::response::Response {
    if let Some(a) = &o.algorithm {
        if parse_algorithm(a).is_none() {
            return err_json(
                400,
                &format!("unknown algorithm {a:?} (round_robin|least_connections|random|failover)"),
            );
        }
    }
    // apply live where possible
    if let Some(pool) = state.tcp_pools.iter().find(|p| p.name == name) {
        apply_override_live(pool, &o);
    } else if state.tcp_pools.iter().any(|p| p.name == name) {
        // unreachable; kept for clarity
    } else if !name.is_empty() && state.tcp_pools.is_empty() {
        return err_json(404, "no tcp pools running");
    }
    let mut map = lock_overrides(&state);
    let entry = map.entry(name.clone()).or_default();
    if o.algorithm.is_some() {
        entry.algorithm = o.algorithm.clone();
    }
    if o.max_conns.is_some() {
        entry.max_conns = o.max_conns;
    }
    if o.drain.is_some() {
        entry.drain = o.drain;
    }
    if o.hc_interval_secs.is_some() {
        entry.hc_interval_secs = o.hc_interval_secs;
    }
    if o.hc_timeout_secs.is_some() {
        entry.hc_timeout_secs = o.hc_timeout_secs;
    }
    drop(map);
    match save_pool_overrides(&state) {
        Ok(()) => axum::Json(serde_json::json!({ "ok": true })).into_response(),
        Err(e) => err_json(500, &e),
    }
}

async fn api_lb(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
) -> axum::Json<serde_json::Value> {
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

async fn api_rib(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
) -> axum::Json<serde_json::Value> {
    match &state.bgp {
        Some(bgp) => {
            let rib = bgp.rib.read().await;
            let mut routes: Vec<serde_json::Value> = rib
                .iter()
                .flat_map(|(p, e)| {
                    e.routes.iter().map(move |r| {
                        serde_json::json!({
                            "prefix": p.to_string(),
                            "neighbor": r.neighbor,
                            "as_path_len": r.as_path_len,
                            "best": e.best().map(|b| b.neighbor == r.neighbor).unwrap_or(false),
                        })
                    })
                })
                .collect();
            routes.sort_by_key(|r| r["prefix"].as_str().unwrap_or("").to_string());
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

/// Constant-time comparison to prevent timing attacks.
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Resolve the API token: env `SDWANLITE_API_TOKEN` overrides config.
fn resolve_token(state: &AppState) -> Option<String> {
    if let Ok(env_token) = std::env::var("SDWANLITE_API_TOKEN") {
        if !env_token.is_empty() {
            return Some(env_token);
        }
    }
    state.config.general.api_token.clone()
}

fn authorized(state: &AppState, headers: &axum::http::HeaderMap) -> bool {
    let Some(provided) = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    else {
        return false;
    };
    // Bearer token (script access) — env overrides config.
    if let Some(token) = resolve_token(state) {
        let expected = format!("Bearer {token}");
        if ct_eq(provided.as_bytes(), expected.as_bytes()) {
            return true;
        }
    }
    // Basic admin credentials (browser UI) — same source as the middleware.
    let user = std::env::var("SDWANLITE_AUTH_USER").unwrap_or_default();
    let pass = std::env::var("SDWANLITE_AUTH_PASS").unwrap_or_default();
    if !user.is_empty() && !pass.is_empty() {
        use base64::Engine as _;
        let expected = format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(format!("{user}:{pass}"))
        );
        if ct_eq(provided.as_bytes(), expected.as_bytes()) {
            return true;
        }
    }
    false
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
    let Some(addr) = body
        .get("addr")
        .and_then(|a| a.as_str())
        .and_then(|a| a.parse().ok())
    else {
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
    let Some(addr) = body
        .get("addr")
        .and_then(|a| a.as_str())
        .and_then(|a| a.parse().ok())
    else {
        return axum::Json(serde_json::json!({ "ok": false, "error": "invalid addr" }));
    };
    let removed = pool.remove_backend(addr).await;
    axum::Json(serde_json::json!({ "ok": removed }))
}

/// Prometheus text exposition of pool/backend metrics.
async fn api_metrics(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
) -> axum::response::Html<String> {
    let mut out = String::with_capacity(4096);
    out.push_str("# HELP sdwanlite_backend_healthy Backend reachability (1 healthy, 0 down).\n");
    out.push_str("# TYPE sdwanlite_backend_healthy gauge\n");
    out.push_str("# HELP sdwanlite_backend_conns Connection counters per backend.\n");
    out.push_str("# TYPE sdwanlite_backend_conns counter\n");
    out.push_str("# HELP sdwanlite_backend_latency_us Backend connect latency percentiles in microseconds.\n");
    out.push_str("# TYPE sdwanlite_backend_latency_us gauge\n");
    out.push_str("# HELP sdwanlite_pool_active_conns Active connections at pool level.\n");
    out.push_str("# TYPE sdwanlite_pool_active_conns gauge\n");
    out.push_str("# HELP sdwanlite_pool_max_conns Pool connection limit (0 = unlimited).\n");
    out.push_str("# TYPE sdwanlite_pool_max_conns gauge\n");

    for pool in &state.tcp_pools {
        for b in pool.backends().await {
            let lbl = format!("{{pool=\"tcp:{}\",backend=\"{}\"}}", pool.name, b.addr);
            out.push_str(&format!(
                "sdwanlite_backend_healthy{lbl} {}\n",
                b.is_healthy() as u8
            ));
            out.push_str(&format!(
                "sdwanlite_backend_conns{{pool=\"tcp:{}\",backend=\"{}\",kind=\"active\"}} {}\n",
                pool.name,
                b.addr,
                b.active_conns()
            ));
            out.push_str(&format!(
                "sdwanlite_backend_conns{{pool=\"tcp:{}\",backend=\"{}\",kind=\"total\"}} {}\n",
                pool.name,
                b.addr,
                b.total_conns()
            ));
            out.push_str(&format!(
                "sdwanlite_backend_bytes{{pool=\"tcp:{}\",backend=\"{}\",dir=\"rx\"}} {}\n",
                pool.name,
                b.addr,
                b.rx_bytes()
            ));
            out.push_str(&format!(
                "sdwanlite_backend_bytes{{pool=\"tcp:{}\",backend=\"{}\",dir=\"tx\"}} {}\n",
                pool.name,
                b.addr,
                b.tx_bytes()
            ));
            let (p50, p95) = b.latency_percentiles_us();
            if let Some(v) = p50 {
                out.push_str(&format!(
                    "sdwanlite_backend_latency_us{{pool=\"tcp:{}\",backend=\"{}\",quantile=\"p50\"}} {}\n",
                    pool.name, b.addr, v
                ));
            }
            if let Some(v) = p95 {
                out.push_str(&format!(
                    "sdwanlite_backend_latency_us{{pool=\"tcp:{}\",backend=\"{}\",quantile=\"p95\"}} {}\n",
                    pool.name, b.addr, v
                ));
            }
        }
        out.push_str(&format!(
            "sdwanlite_pool_rejected{{pool=\"tcp:{}\"}} {}\n",
            pool.name,
            pool.rejected_conns()
        ));
        out.push_str(&format!(
            "sdwanlite_pool_active_conns{{pool=\"tcp:{}\"}} {}\n",
            pool.name,
            pool.active_conns()
        ));
        out.push_str(&format!(
            "sdwanlite_pool_max_conns{{pool=\"tcp:{}\"}} {}\n",
            pool.name,
            pool.max_conns()
        ));
    }
    for pool in &state.http_pools {
        for backends in pool.backends_by_route() {
            for b in &backends {
                let lbl = format!("{{pool=\"http:{}\",backend=\"{}\"}}", pool.name, b.addr);
                out.push_str(&format!(
                    "sdwanlite_backend_healthy{lbl} {}\n",
                    b.is_healthy() as u8
                ));
                out.push_str(&format!("sdwanlite_backend_conns{{pool=\"http:{}\",backend=\"{}\",kind=\"total\"}} {}\n", pool.name, b.addr, b.total_conns()));
            }
        }
    }

    let (rib_size, sessions_est) = match &state.bgp {
        Some(b) => {
            let rib_size = b.rib.read().await.len();
            let sessions_est = b
                .sessions
                .read()
                .await
                .values()
                .filter(|i| i.state == sdwanlite_bgp::SessionState::Established)
                .count() as u64;
            (rib_size, sessions_est)
        }
        None => (0, 0),
    };
    out.push_str("# HELP sdwanlite_bgp_rib_routes Current best-path RIB size.\n# TYPE sdwanlite_bgp_rib_routes gauge\n");
    out.push_str(&format!("sdwanlite_bgp_rib_routes {rib_size}\n"));
    out.push_str("# HELP sdwanlite_bgp_sessions_established Established BGP sessions.\n# TYPE sdwanlite_bgp_sessions_established gauge\n");
    out.push_str(&format!(
        "sdwanlite_bgp_sessions_established {sessions_est}\n"
    ));
    out.push_str(&format!(
        "# HELP sdwanlite_uptime_seconds Process uptime in seconds.\n# TYPE sdwanlite_uptime_seconds counter\nsdwanlite_uptime_seconds {}\n",
        state.started.elapsed().as_secs()
    ));
    axum::response::Html(out)
}

/// Re-read the config file and synchronise TCP-pool backends with it.
/// Listener/TLS/BGP changes still require a process restart; the response
/// reports what was applied and what needs a restart.
async fn api_reload(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
) -> axum::Json<serde_json::Value> {
    if !authorized(&state, &headers) {
        return axum::Json(serde_json::json!({ "ok": false, "error": "unauthorized" }));
    }
    let path = std::path::PathBuf::from("sdwanlite.toml");
    let new_cfg = match sdwanlite_core::Config::load(&path) {
        Ok(c) => c,
        Err(e) => return axum::Json(serde_json::json!({ "ok": false, "error": e.to_string() })),
    };

    let mut applied: Vec<serde_json::Value> = Vec::new();
    for desired in &new_cfg.lb.tcp_pools {
        if let Some(pool) = state.tcp_pools.iter().find(|p| p.name == desired.name) {
            // algorithm / limits live-update where possible
            let want: Vec<std::net::SocketAddr> = desired
                .backends
                .iter()
                .filter_map(|s| s.parse().ok())
                .collect();
            let current = pool.backends().await;
            for w in &want {
                if !current.iter().any(|c| c.addr == *w) {
                    pool.add_backend(*w).await;
                    applied.push(serde_json::json!({"pool": desired.name, "added": w.to_string()}));
                }
            }
            for c in &current {
                if !want.contains(&c.addr) {
                    pool.remove_backend(c.addr).await;
                    applied.push(
                        serde_json::json!({"pool": desired.name, "removed": c.addr.to_string()}),
                    );
                }
            }
        } else {
            applied.push(serde_json::json!({
                "restart_required": true,
                "reason": format!("new pool '{}' requires process restart", desired.name)
            }));
        }
    }

    axum::Json(serde_json::json!({
        "ok": true,
        "applied": applied,
        "note": "listener/tls/bgp changes require restart"
    }))
}

/// Rebuild TLS acceptors for HTTP pools whose config has a TLS section.
async fn api_tls_reload(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
) -> axum::Json<serde_json::Value> {
    if !authorized(&state, &headers) {
        return axum::Json(serde_json::json!({ "ok": false, "error": "unauthorized" }));
    }
    let mut results = Vec::new();
    for pool_cfg in &state.config.lb.http_pools {
        let Some(tls_cfg) = &pool_cfg.tls else {
            continue;
        };
        let Some(pool) = state.http_pools.iter().find(|p| p.name == pool_cfg.name) else {
            continue;
        };
        match pool.reload_tls(tls_cfg).await {
            Ok(()) => results.push(serde_json::json!({ "pool": pool_cfg.name, "reloaded": true })),
            Err(e) => results.push(serde_json::json!({ "pool": pool_cfg.name, "reloaded": false, "error": e.to_string() })),
        }
    }
    axum::Json(serde_json::json!({ "ok": true, "results": results }))
}

/// Certificate + ACME status for the TLS management view.
/// Never returns the Cloudflare token value — only whether it is set.
async fn api_tls_status(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
) -> axum::Json<serde_json::Value> {
    let acme = &state.config.acme;
    let cert_path = std::path::Path::new(&acme.cert_file);
    let cert = if cert_path.exists() {
        match std::fs::read(cert_path) {
            Ok(data) => x509_parser::prelude::parse_x509_pem(&data)
                .ok()
                .and_then(|(_, pem)| {
                    use x509_parser::prelude::FromDer as _;
                    let (_, c) =
                        x509_parser::prelude::X509Certificate::from_der(&pem.contents).ok()?;
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs() as i64)
                        .unwrap_or(0);
                    let not_before: i64 = c.validity().not_before.timestamp();
                    let not_after: i64 = c.validity().not_after.timestamp();
                    let sans = c
                        .subject_alternative_name()
                        .ok()
                        .flatten()
                        .map(|san| {
                            san.value
                                .general_names
                                .iter()
                                .filter_map(|g| match g {
                                    x509_parser::extensions::GeneralName::DNSName(n) => {
                                        Some(n.to_string())
                                    }
                                    _ => None,
                                })
                                .collect::<Vec<String>>()
                        })
                        .unwrap_or_default();
                    Some(serde_json::json!({
                        "subject": c.subject().to_string(),
                        "issuer": c.issuer().to_string(),
                        "not_before": not_before,
                        "not_after": not_after,
                        "days_left": (not_after - now).div_euclid(86400).max(0),
                        "sans": sans,
                    }))
                })
                .unwrap_or_else(
                    || serde_json::json!({ "parse_error": "not a valid PEM certificate" }),
                ),
            Err(e) => serde_json::json!({ "read_error": e.to_string() }),
        }
    } else {
        serde_json::Value::Null
    };
    axum::Json(serde_json::json!({
        "cert_file": acme.cert_file,
        "key_file": acme.key_file,
        "cert": cert,
        "acme": {
            "enabled": acme.enabled,
            "directory_url": acme.directory_url,
            "email": acme.email,
            "domains": acme.domains,
            "http01_port": acme.http01_port,
            "renew_days": acme.renew_days,
            "dns01": acme.dns01,
            "cloudflare_token_set": acme.cloudflare_api_token.is_some(),
        },
    }))
}

/// Kick off an ACME issue in the background; the certificate is written to
/// the configured cert_file on success (then call /api/tls/reload to pick
/// it up in the HTTP pools).
async fn api_acme_issue(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
) -> axum::Json<serde_json::Value> {
    if !authorized(&state, &headers) {
        return axum::Json(serde_json::json!({ "ok": false, "error": "unauthorized" }));
    }
    let cfg = state.config.acme.clone();
    if !cfg.enabled {
        return axum::Json(serde_json::json!({
            "ok": false,
            "error": "acme is disabled in sdwanlite.toml (acme.enabled = false)"
        }));
    }
    tokio::spawn(async move {
        match sdwanlite_acme::renew::issue_now(&cfg).await {
            Ok(()) => tracing::info!("acme: certificate issued and written"),
            Err(e) => tracing::error!("acme: issue failed: {e}"),
        }
    });
    axum::Json(serde_json::json!({
        "ok": true,
        "started": true,
        "note": "issue started in background; run TLS reload after it completes"
    }))
}

/// Server-Sent Events stream of the status payload (replaces dashboard polling).
async fn api_events(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
) -> axum::response::Sse<
    impl futures_core::Stream<Item = Result<axum::response::sse::Event, std::convert::Infallible>>,
> {
    use axum::response::sse::{Event, KeepAlive};
    let interval = tokio_stream::wrappers::IntervalStream::new(tokio::time::interval(
        std::time::Duration::from_secs(3),
    ));
    let state = state.clone();
    let stream = interval.map(move |_| {
        let state = &state;
        // build the same payload as /api/status synchronously enough for SSE
        let node = state.config.general.name.clone();
        let uptime = state.started.elapsed().as_secs();
        let tcp_pools = state.tcp_pools.len();
        let http_pools = state.http_pools.len();
        let ev = Event::default().data(
            serde_json::json!({
                "node": node,
                "uptime_secs": uptime,
                "lb": { "tcp_pools": tcp_pools, "http_pools": http_pools }
            })
            .to_string(),
        );
        Ok(ev)
    });
    axum::response::Sse::new(stream).keep_alive(KeepAlive::default())
}

// ---------------------------------------------------------------------------
// Alerts + Firewall API
// ---------------------------------------------------------------------------

async fn api_alerts(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
) -> axum::Json<serde_json::Value> {
    let events = state.alerts.list();
    let list: Vec<serde_json::Value> = events.iter().map(|e| {
        serde_json::json!({ "timestamp": e.timestamp, "severity": e.severity, "source": e.source, "message": e.message })
    }).collect();
    axum::Json(serde_json::json!({ "count": list.len(), "events": list }))
}

async fn api_firewall_list(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
) -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({ "rules": state.config.firewall }))
}

async fn api_firewall_add(
    axum::extract::State(_state): axum::extract::State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    axum::Json(_rule): axum::Json<serde_json::Value>,
) -> axum::Json<serde_json::Value> {
    if !authorized(&_state, &headers) {
        return axum::Json(serde_json::json!({ "ok": false, "error": "unauthorized" }));
    }
    // firewall rules are read from config; dynamic add requires restart
    axum::Json(
        serde_json::json!({ "ok": false, "error": "firewall rules are config-managed; add to sdwanlite.toml and restart" }),
    )
}

async fn api_firewall_delete(
    axum::extract::State(_state): axum::extract::State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    axum::Json(_body): axum::Json<serde_json::Value>,
) -> axum::Json<serde_json::Value> {
    if !authorized(&_state, &headers) {
        return axum::Json(serde_json::json!({ "ok": false, "error": "unauthorized" }));
    }
    axum::Json(
        serde_json::json!({ "ok": false, "error": "firewall rules are config-managed; edit sdwanlite.toml and restart" }),
    )
}

async fn api_validate(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
) -> axum::Json<serde_json::Value> {
    let errors = sdwanlite_core::validate_config(&state.config);
    if errors.is_empty() {
        axum::Json(serde_json::json!({ "valid": true, "errors": [] }))
    } else {
        axum::Json(serde_json::json!({ "valid": false, "errors": errors }))
    }
}
