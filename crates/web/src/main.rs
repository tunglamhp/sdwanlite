//! sdwanlite-web: Dioxus/WASM frontend for the control panel.

use dioxus::prelude::*;
use serde::{Deserialize, Serialize};
use std::future::Future;

// ---------------------------------------------------------------------------
// localStorage helpers
// ---------------------------------------------------------------------------

fn ls_get(key: &str) -> Option<String> {
    let storage = web_sys::window()?.local_storage().ok()??;
    storage.get_item(key).ok()?
}

fn ls_set(key: &str, value: &str) {
    if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        let _ = storage.set_item(key, value);
    }
}

fn apply_theme(theme: &str) {
    if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
        if let Some(el) = doc.document_element() {
            let _ = el.set_attribute("data-theme", theme);
        }
    }
}

fn fmt_uptime(s: u64) -> String {
    format!("{}h {}m {}s", s / 3600, (s % 3600) / 60, s % 60)
}

// ---------------------------------------------------------------------------
// API types
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default, Deserialize)]
struct Status {
    node: String,
    version: String,
    uptime_secs: u64,
    mesh_enabled: bool,
    mesh_peers_configured: usize,
    bgp_enabled: bool,
    #[serde(default)]
    bgp_sessions: Vec<BgpSession>,
    #[serde(default)]
    bgp_rib_size: usize,
    #[serde(default)]
    lb: LbCounts,
    #[serde(default)]
    auth_enabled: bool,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct BgpSession {
    neighbor: String,
    state: String,
    #[serde(default)]
    remote_as: Option<u32>,
    #[serde(default)]
    prefixes_received: u64,
    #[serde(default)]
    flaps: u64,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct LbCounts {
    #[serde(default)]
    tcp_pools: usize,
    #[serde(default)]
    http_pools: usize,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct LbData {
    #[serde(default)]
    tcp: Vec<TcpPool>,
    #[serde(default)]
    http: Vec<HttpPool>,
}

#[derive(Clone, Debug, Deserialize)]
struct TcpPool {
    name: String,
    algorithm: String,
    #[serde(default)]
    active_conns: u64,
    #[serde(default)]
    rejected_conns: u64,
    #[serde(default)]
    backends: Vec<Backend>,
}

#[derive(Clone, Debug, Deserialize)]
struct Backend {
    addr: String,
    healthy: bool,
    #[serde(default)]
    active_conns: u64,
    #[serde(default)]
    total_conns: u64,
}

#[derive(Clone, Debug, Deserialize)]
struct SignalBackend {
    addr: String,
    healthy: bool,
    #[serde(default)]
    latency_p50_us: Option<u64>,
    #[serde(default)]
    latency_p95_us: Option<u64>,
    #[serde(default)]
    rx_bytes: u64,
    #[serde(default)]
    tx_bytes: u64,
    #[serde(default)]
    active_conns: u64,
    #[serde(default)]
    health_failures: u64,
}

#[derive(Clone, Debug, Deserialize)]
struct SignalPool {
    name: String,
    #[serde(default)]
    latency_p50_us: Option<u64>,
    #[serde(default)]
    latency_p95_us: Option<u64>,
    #[serde(default)]
    rx_bytes: u64,
    #[serde(default)]
    tx_bytes: u64,
    #[serde(default)]
    errors: SignalErrors,
    #[serde(default)]
    saturation: SignalSaturation,
    #[serde(default)]
    backends: Vec<SignalBackend>,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct SignalErrors {
    #[serde(default)]
    unhealthy_backends: usize,
    #[serde(default)]
    total_backends: usize,
    #[serde(default)]
    loss_pct: f64,
    #[serde(default)]
    health_failures: u64,
    #[serde(default)]
    rejected_conns: u64,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct SignalSaturation {
    #[serde(default)]
    active_conns: usize,
    #[serde(default)]
    max_conns: usize,
    #[serde(default)]
    utilization_pct: f64,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct Signals {
    #[serde(default)]
    pools: Vec<SignalPool>,
    #[serde(default)]
    totals: SignalTotals,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct SignalTotals {
    #[serde(default)]
    latency_p50_us: Option<u64>,
    #[serde(default)]
    latency_p95_us: Option<u64>,
    #[serde(default)]
    rx_bytes: u64,
    #[serde(default)]
    tx_bytes: u64,
    #[serde(default)]
    unhealthy_backends: usize,
    #[serde(default)]
    total_backends: usize,
    #[serde(default)]
    health_failures: u64,
    #[serde(default)]
    rejected_conns: u64,
    #[serde(default)]
    active_conns: usize,
    #[serde(default)]
    max_conns: usize,
    #[serde(default)]
    utilization_pct: f64,
}

#[derive(Clone, Debug, Deserialize)]
struct HttpPool {
    name: String,
    #[serde(default)]
    routes: Vec<HttpRoute>,
}

#[derive(Clone, Debug, Deserialize)]
struct HttpRoute {
    #[serde(default)]
    host: String,
    #[serde(default)]
    path_prefix: String,
    #[serde(default)]
    backends: usize,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct MeshStatus {
    available: bool,
    #[serde(default)]
    peers: Vec<WgPeer>,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct WgPeer {
    public_key: String,
    endpoint: Option<String>,
    latest_handshake_secs_ago: Option<u64>,
    rx_bytes: u64,
    tx_bytes: u64,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct RibData {
    count: usize,
    #[serde(default)]
    routes: Vec<RibRoute>,
}

#[derive(Clone, Debug, Deserialize)]
struct RibRoute {
    prefix: String,
    neighbor: String,
    #[serde(default)]
    as_path_len: Option<u64>,
    #[serde(default)]
    best: bool,
}

#[derive(Clone, Debug, Deserialize)]
struct Keypair {
    private_key: String,
    public_key: String,
}

// ---------------------------------------------------------------------------
// API client
// ---------------------------------------------------------------------------

mod api {
    use super::*;
    use gloo_net::http::Request;

    async fn get_json<T: for<'de> Deserialize<'de>>(url: &str) -> Result<T, String> {
        let rsp = Request::get(url).send().await.map_err(|e| e.to_string())?;
        if !rsp.ok() {
            return Err(format!("HTTP {}", rsp.status()));
        }
        rsp.json::<T>().await.map_err(|e| e.to_string())
    }

    pub fn status() -> impl Future<Output = Result<Status, String>> { get_json("/api/status") }
    pub fn lb() -> impl Future<Output = Result<LbData, String>> { get_json("/api/lb") }
    pub fn mesh_status() -> impl Future<Output = Result<MeshStatus, String>> { get_json("/api/mesh/status") }
    pub fn rib() -> impl Future<Output = Result<RibData, String>> { get_json("/api/bgp/rib") }
    pub fn keypair() -> impl Future<Output = Result<Keypair, String>> { get_json("/api/mesh/keypair") }

    pub async fn call(method: &str, url: &str) -> Result<String, String> {
        let builder = match method {
            "POST" => Request::post(url),
            "DELETE" => Request::delete(url),
            "PUT" => Request::put(url),
            _ => Request::get(url),
        };
        let rsp = builder.send().await.map_err(|e| e.to_string())?;
        let text = rsp.text().await.map_err(|e| e.to_string())?;
        Ok(format!("HTTP {} — {}", rsp.status(), text))
    }
}

// ---------------------------------------------------------------------------
// shared UI helpers
// ---------------------------------------------------------------------------

fn pill(ok: bool, ok_text: &str, bad_text: &str) -> Element {
    rsx! {
        span { class: if ok { "pill ok" } else { "pill bad" }, {if ok { ok_text } else { bad_text }} }
    }
}

#[component]
fn kpi_card(label: &'static str, value: String, sub: String) -> Element {
    rsx! {
        div { class: "kpi",
            div { class: "label", "{label}" }
            div { class: "value", "{value}" }
            div { class: "sub", "{sub}" }
        }
    }
}

// ---------------------------------------------------------------------------
// App
// ---------------------------------------------------------------------------

fn app() -> Element {
    let mut tab = use_signal(|| "overview".to_string());
    let mut theme: Signal<String> = use_signal(|| ls_get("sl-theme").unwrap_or_else(|| "dark".to_string()));

    let mut status: Signal<Result<Status, String>> = use_signal(|| Err("loading…".into()));
    let mut lb: Signal<Result<LbData, String>> = use_signal(|| Err("loading…".into()));
    let mut mesh: Signal<Result<MeshStatus, String>> = use_signal(|| Err("loading…".into()));
    let mut rib: Signal<Result<RibData, String>> = use_signal(|| Err("loading…".into()));
    let mut rib_hist: Signal<Vec<usize>> = use_signal(Vec::new);
    let mut auto_refresh: Signal<bool> = use_signal(|| true);
    let mut sidebar_open: Signal<bool> = use_signal(|| false);

    use_effect(move || {
        spawn(async move {
            loop {
                status.set(api::status().await);
                lb.set(api::lb().await);
                gloo_timers::future::TimeoutFuture::new(3000).await;
            }
        });
    });

    use_effect(move || {
        spawn(async move {
            loop {
                mesh.set(api::mesh_status().await);
                let r = api::rib().await;
                if let Ok(data) = &r {
                    let mut hist = rib_hist.write();
                    hist.push(data.count);
                    if hist.len() > 60 {
                        hist.remove(0);
                    }
                }
                rib.set(r);
                gloo_timers::future::TimeoutFuture::new(5000).await;
            }
        });
    });

    rsx! {
        div { class: "layout",
            {if sidebar_open() { rsx! {
                div { class: "sidebar-overlay", onclick: move |_| sidebar_open.set(false) }
            }} else { rsx! {} }}
            // ---- Sidebar ----
            div { class: if sidebar_open() { "sidebar open" } else { "sidebar" },
                div { class: "sidebar-header",
                    div { class: "sidebar-logo", "SL" }
                    div { b { "SDWANLite" } small { "CONTROL PANEL" } }
                }
                div { class: "sidebar-nav",
                    for (id, icon, label) in [
                        ("overview", "◉", "Overview"),
                        ("topology", "⬡", "Topology"),
                        ("mesh", "🔒", "Mesh"),
                        ("bgp", "🌐", "BGP"),
                        ("lb", "⚖", "Load Balancers"),
                        ("actions", "⚡", "Actions"),
                        ("firewall", "🛡", "Firewall"),
                        ("alerts", "🔔", "Alerts"),
                        ("qos", "📊", "QoS"),
                    ] {
                        button {
                            key: "{id}",
                            class: if tab() == id { "active" } else { "" },
                            onclick: move |_| tab.set(id.to_string()),
                            span { class: "icon", "{icon}" }
                            "{label}"
                        }
                    }
                }
                div { class: "sidebar-footer",
                    div { class: "live", span { class: "live-dot" } "LIVE" }
                    button {
                        class: "icon-btn",
                        title: "Toggle auto-refresh",
                        style: if auto_refresh() { "color:var(--primary-light)" } else { "color:var(--muted)" },
                        onclick: move |_| auto_refresh.toggle(),
                        {if auto_refresh() { "⏸" } else { "▶" }}
                    }
                    button {
                        class: "icon-btn",
                        title: "Toggle light/dark",
                        onclick: move |_| {
                            let next = if theme() == "dark" { "light" } else { "dark" };
                            theme.set(next.to_string());
                            ls_set("sl-theme", &next);
                            apply_theme(&next);
                        },
                        {if theme() == "dark" { "☀" } else { "🌙" }}
                    }
                }
            }
            // ---- Main content ----
            div { class: "main",
                div { class: "main-header",
                    h2 { {match tab().as_str() {
                        "topology" => "Network Topology",
                        "mesh" => "WireGuard Mesh",
                        "bgp" => "BGP",
                        "lb" => "Load Balancers",
                        "actions" => "Quick Actions",
                        _ => "Dashboard Overview",
                    }} }
                    button { class: "hamburger", onclick: move |_| sidebar_open.toggle(), "☰" }
                div { class: "spacer" }
                }
                {match tab().as_str() {
                    "topology" => rsx! { TopologyView { lb } },
                    "mesh" => rsx! { MeshView { status, mesh } },
                    "bgp" => rsx! { BgpView { status, rib, rib_hist } },
                    "lb" => rsx! { LbView { lb } },
                    "actions" => rsx! { ActionsView {} },
                "firewall" => rsx! { FirewallView {} },
                "alerts" => rsx! { AlertsView {} },
                "qos" => rsx! { QosView {} },
                    _ => rsx! { Overview { status } },
                }}
                footer { "sdwanlite · data auto-refreshes · built with Dioxus" }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Overview
// ---------------------------------------------------------------------------

fn fmt_bps(bytes: u64) -> String {
    // cumulative bytes -> human; bps computed by caller with delta
    let v = bytes as f64;
    if v >= 1_073_741_824.0 { format!("{:.1} GiB", v / 1_073_741_824.0) }
    else if v >= 1_048_576.0 { format!("{:.1} MiB", v / 1_048_576.0) }
    else if v >= 1024.0 { format!("{:.1} KiB", v / 1024.0) }
    else { format!("{} B", bytes) }
}

fn fmt_us(us: Option<u64>) -> String {
    match us {
        None => "—".into(),
        Some(x) if x >= 1_000_000 => format!("{:.2} s", x as f64 / 1e6),
        Some(x) if x >= 1_000 => format!("{:.1} ms", x as f64 / 1e3),
        Some(x) => format!("{} µs", x),
    }
}

#[component]
fn GoldenSignalsRow() -> Element {
    let mut prev = use_signal(|| (0u64, 0u64, 0u64)); // rx, tx, timestamp_ms
    let mut bps = use_signal(|| (0u64, 0u64)); // in, out
    let data = use_resource(move || async move {
        let rsp = gloo_net::http::Request::get("/api/signals").send().await;
        match rsp {
            Ok(r) => r.json::<Signals>().await.map_err(|e| e.to_string()),
            Err(e) => Err(e.to_string()),
        }
    });
    // compute bps delta after each fetch
    use_effect(move || {
        let d = data.read();
        if let Some(Ok(sig)) = d.as_ref() {
            let now = web_sys::window().map(|w| w.performance().map(|p| p.now() as u64).unwrap_or(0)).unwrap_or(0);
            let (prx, ptx, pts) = *prev.read();
            let (rx, tx) = (sig.totals.rx_bytes, sig.totals.tx_bytes);
            if pts > 0 && now > pts {
                let dt = (now - pts) / 1000;
                if dt > 0 {
                    bps.set((rx.saturating_sub(prx) / dt, tx.saturating_sub(ptx) / dt));
                }
            }
            prev.set((rx, tx, now));
        }
    });

    let sig = match data.read().as_ref() {
        Some(Ok(s)) => s.clone(),
        _ => Signals::default(),
    };
    let t = &sig.totals;
    let err_pill = if t.total_backends == 0 {
        "<span class='pill info'>no backends</span>".to_string()
    } else if t.unhealthy_backends == 0 {
        "<span class='pill ok'>all healthy</span>".to_string()
    } else {
        format!("<span class='pill bad'>{}/{} down</span>", t.unhealthy_backends, t.total_backends)
    };
    let sat_pill = if t.max_conns == 0 {
        "<span class='pill info'>unlimited</span>".to_string()
    } else if t.utilization_pct >= 80.0 {
        format!("<span class='pill bad'>{:.0}%</span>", t.utilization_pct)
    } else {
        format!("<span class='pill ok'>{:.0}%</span>", t.utilization_pct)
    };

    rsx! {
        div { class: "golden-signals",
            div { class: "kpi",
                style: "border-left-color:var(--amber)",
                div { class: "label", "LATENCY" }
                div { class: "value", "{fmt_us(t.latency_p50_us)}" }
                div { class: "sub", "p95 {fmt_us(t.latency_p95_us)} · connect p50/p95" }
            }
            div { class: "kpi",
                style: "border-left-color:var(--primary)",
                div { class: "label", "TRAFFIC" }
                div { class: "value", "↓ {fmt_bps(bps.read().0)}" }
                div { class: "sub", "↑ {fmt_bps(bps.read().1)} · total {fmt_bps(t.rx_bytes + t.tx_bytes)}" }
            }
            div { class: "kpi",
                style: "border-left-color:var(--red)",
                div { class: "label", "ERRORS" }
                div { class: "value", dangerous_inner_html: "{err_pill}" }
                div { class: "sub", "hc failures {t.health_failures} · rejected {t.rejected_conns}" }
            }
            div { class: "kpi",
                style: "border-left-color:var(--accent)",
                div { class: "label", "SATURATION" }
                div { class: "value", dangerous_inner_html: "{sat_pill}" }
                div { class: "sub",
                    "{t.active_conns} conns / max ",
                    if t.max_conns == 0 { "∞" } else { "{t.max_conns}" },
                }
            }
        }
    }
}

#[component]
fn Overview(status: Signal<Result<Status, String>>) -> Element {
    rsx! {
        GoldenSignalsRow {}
        div { class: "kpis",
            kpi_card { label: "Node",
                value: match &*status.read() { Ok(s) => s.node.clone(), _ => "—".to_string() },
                sub: match &*status.read() { Ok(s) => format!("v{}", s.version), _ => String::new() } }
            kpi_card { label: "Uptime",
                value: match &*status.read() { Ok(s) => fmt_uptime(s.uptime_secs), _ => "—".to_string() },
                sub: "since start".to_string() }
            kpi_card { label: "LB Pools",
                value: match &*status.read() { Ok(s) => (s.lb.tcp_pools + s.lb.http_pools).to_string(), _ => "—".to_string() },
                sub: match &*status.read() { Ok(s) => format!("{} TCP / {} HTTP", s.lb.tcp_pools, s.lb.http_pools), _ => "".into() } }
            kpi_card { label: "BGP RIB",
                value: match &*status.read() { Ok(s) => s.bgp_rib_size.to_string(), _ => "—".to_string() },
                sub: match &*status.read() { Ok(s) => if s.bgp_enabled { "enabled".to_string() } else { "disabled".to_string() }, _ => String::new() } }
        }
        div { class: "grid2",
            div { class: "card",
                h3 { "BGP Sessions" }
                table {
                    thead { tr { th { "Neighbor" } th { "Remote AS" } th { "State" } th { "Prefixes" } th { "Flaps" } } }
                    tbody {
                        {match &*status.read() {
                            Ok(s) if s.bgp_sessions.is_empty() => rsx! {
                                tr { td { colspan: "5", style: "color:var(--muted)", "no sessions" } }
                            },
                            Ok(s) => rsx! {
                                {s.bgp_sessions.iter().map(|x| rsx! {
                                    tr { key: "{x.neighbor}",
                                        td { class: "mono", "{x.neighbor}" }
                                        td { {x.remote_as.map(|a| rsx! {"AS{a}"}).unwrap_or_else(|| rsx! {"—"})} }
                                        td { {pill(x.state == "established", &x.state, &x.state)} }
                                        td { "{x.prefixes_received}" }
                                        td { "{x.flaps}" }
                                    }
                                })}
                            },
                            Err(e) => rsx! { tr { td { colspan: "5", style: "color:var(--red)", "{e}" } } },
                        }}
                    }
                }
            }
            div { class: "card",
                h3 { "System" }
                {match &*status.read() {
                    Ok(s) => rsx! {
                        div { class: "kv-row",
                            span { class: "k", "Mesh" }
                            span { class: if s.mesh_enabled {"pill ok"} else {"pill warn"},
                                {if s.mesh_enabled {"enabled"} else {"disabled"}}
                            }
                        }
                        div { class: "kv-row",
                            span { class: "k", "BGP" }
                            span { class: if s.bgp_enabled {"pill ok"} else {"pill warn"},
                                {if s.bgp_enabled {"enabled"} else {"disabled"}}
                            }
                        }
                        div { class: "kv-row",
                            span { class: "k", "WG peers configured" }
                            span { "{s.mesh_peers_configured}" }
                        }
                        div { class: "kv-row",
                            span { class: "k", "Auth" }
                            {if s.auth_enabled { rsx! { span { class: "pill ok", "enabled" } }} else { rsx! { span { class: "pill warn", "dev mode" } }}}
                        }
                    },
                    _ => rsx! {},
                }}
            }
        }
    }
}


#[component]
fn MeshView(
    status: Signal<Result<Status, String>>,
    mesh: Signal<Result<MeshStatus, String>>,
) -> Element {
    let mut keys: Signal<Option<Keypair>> = use_signal(|| None);

    let peers_html = match &*mesh.read() {
        Ok(w) if !w.available => "<div style='color:var(--muted);font-size:13px'>wg tools unavailable on this host — config rendering &amp; keypair APIs still work</div>".to_string(),
        Ok(w) => {
            let mut rows = String::new();
            for p in &w.peers {
                let hs = p.latest_handshake_secs_ago
                    .map(|s| format!("{}s ago", s))
                    .unwrap_or_else(|| "<span class='pill warn'>never</span>".into());
                let key_head: String = p.public_key.chars().take(14).collect();
                rows.push_str(&format!(
                    "<tr><td class='mono'>{}…</td><td>{}</td><td>{} / {}</td><td>{}</td></tr>",
                    key_head,
                    p.endpoint.clone().unwrap_or_else(|| "—".into()),
                    p.rx_bytes, p.tx_bytes, hs
                ));
            }
            if rows.is_empty() {
                "<div style='color:var(--muted);font-size:13px'>no peers</div>".into()
            } else {
                format!(
                    "<table><thead><tr><th>Public key</th><th>Endpoint</th><th>RX / TX</th><th>Handshake</th></tr></thead><tbody>{}</tbody></table>",
                    rows
                )
            }
        }
        Err(e) => format!("<div style='color:var(--red);font-size:13px'>{e}</div>"),
    };

    rsx! {
        div { class: "grid2",
            div { class: "card",
                h3 { "Configuration" }
                {match &*status.read() {
                    Ok(s) => rsx! {
                        div { class: "kv-row",
                            span { class: "k", "Status" }
                            span { class: if s.mesh_enabled {"pill ok"} else {"pill warn"},
                                {if s.mesh_enabled {"enabled"} else {"disabled"}}
                            }
                        }
                        div { class: "kv-row",
                            span { class: "k", "Peers configured" }
                            span { "{s.mesh_peers_configured}" }
                        }
                    },
                    _ => rsx! {},
                }}
                button { class: "btn", onclick: move |_| {
                        spawn(async move {
                            if let Ok(kp) = api::keypair().await {
                                keys.set(Some(kp));
                            }
                        });
                    },
                    "Generate keypair"
                }
                {keys().map(|kp| rsx! {
                    pre { class: "keys", "private = {kp.private_key}\npublic  = {kp.public_key}" }
                })}
            }
            div { class: "card",
                h3 { "Live peers (kernel WG)" }
                div { dangerous_inner_html: "{peers_html}" }
            }
        }
    }
}



// ---------------------------------------------------------------------------
// BGP
// ---------------------------------------------------------------------------

#[component]
fn BgpView(
    status: Signal<Result<Status, String>>,
    rib: Signal<Result<RibData, String>>,
    rib_hist: Signal<Vec<usize>>,
) -> Element {
    let points = rib_points(&*rib_hist.read());

    rsx! {
        div { class: "grid2",
            div { class: "card",
                h3 { "Sessions" }
                table {
                    thead { tr { th { "Neighbor" } th { "Remote AS" } th { "State" } th { "Prefixes" } th { "Flaps" } } }
                    tbody {
                        {match &*status.read() {
                            Ok(s) if s.bgp_sessions.is_empty() => rsx! {
                                tr { td { colspan: "5", style: "color:var(--muted)", "no sessions" } }
                            },
                            Ok(s) => rsx! {
                                {s.bgp_sessions.iter().map(|x| rsx! {
                                    tr { key: "{x.neighbor}",
                                        td { class: "mono", "{x.neighbor}" }
                                        td { {x.remote_as.map(|a| rsx! {"AS{a}"}).unwrap_or_else(|| rsx! {"—"})} }
                                        td { {pill(x.state == "established", &x.state, &x.state)} }
                                        td { "{x.prefixes_received}" }
                                        td { "{x.flaps}" }
                                    }
                                })}
                            },
                            Err(e) => rsx! { tr { td { colspan: "5", style: "color:var(--red)", "{e}" } } },
                        }}
                    }
                }
            }
            div { class: "card",
                h3 { "RIB" }
                div { class: "kv-row",
                    span { class: "k", "entries" }
                    b { {match &*rib.read() { Ok(r) => r.count.to_string(), _ => "—".to_string() }} }
                }
                svg { width: "100%", height: "56",
                    polyline {
                        fill: "none",
                        stroke: "var(--accent)",
                        stroke_width: "2",
                        points: "{points}",
                    }
                }
                div { style: "color:var(--muted);font-size:10px;letter-spacing:1px;margin-top:4px",
                    "RIB HISTORY (LIVE)"
                }
                table { style: "margin-top:10px",
                    thead { tr { th { "Prefix" } th { "Neighbor" } th { "AS-path" } th { "Best" } } }
                    tbody {
                        {match &*rib.read() {
                            Ok(r) if r.routes.is_empty() => rsx! {
                                tr { td { colspan: "4", style: "color:var(--muted)", "empty" } }
                            },
                            Ok(r) => rsx! {
                                {r.routes.iter().map(|rt| rsx! {
                                    tr { key: "{rt.prefix}-{rt.neighbor}",
                                        td { class: "mono", "{rt.prefix}" }
                                        td { "{rt.neighbor}" }
                                        td { {rt.as_path_len.map(|l| rsx! {"{l}"}).unwrap_or_else(|| rsx! {"—"})} }
                                        td { {if rt.best { rsx! { span { class: "pill ok", "best" } }} else { rsx! {} }} }
                                    }
                                })}
                            },
                            Err(e) => rsx! { tr { td { colspan: "4", style: "color:var(--red)", "{e}" } } },
                        }}
                    }
                }
            }
        }
    }
}

fn rib_points(hist: &[usize]) -> String {
    if hist.len() < 2 {
        return String::new();
    }
    let max = hist.iter().copied().max().unwrap_or(1).max(1);
    let w = 300.0f64;
    let h = 48.0f64;
    hist.iter()
        .enumerate()
        .map(|(i, v)| {
            let x = (i as f64 / (hist.len() - 1) as f64) * (w - 4.0) + 2.0;
            let y = h - 4.0 - (*v as f64 / max as f64) * (h - 10.0);
            format!("{x:.1},{y:.1}")
        })
        .collect::<Vec<String>>()
        .join(" ")
}

// ---------------------------------------------------------------------------
// Load balancers
// ---------------------------------------------------------------------------


// ---- Deep config modal (flexiWAN-style) ----
#[derive(Clone, PartialEq)]
enum ConfigTab { General, HealthCheck, Advanced }

#[component]
fn PoolConfigModal(pool_name: String, algorithm: String, on_close: EventHandler<()>) -> Element {
    let mut tab = use_signal(|| ConfigTab::General);
    let mut hc_interval = use_signal(|| "5".to_string());
    let mut hc_timeout = use_signal(|| "3".to_string());
    let mut hc_retries = use_signal(|| "3".to_string());
    let mut hc_path = use_signal(|| "/health".to_string());
    let mut conn_timeout = use_signal(|| "30".to_string());
    let mut max_conns = use_signal(|| "1000".to_string());
    let mut drain_mode = use_signal(|| false);

    let tab_name = |t: &ConfigTab| match t {
        ConfigTab::General => "General",
        ConfigTab::HealthCheck => "Health Check",
        ConfigTab::Advanced => "Advanced",
    };

    rsx! {
        div {
            class: "modal-overlay",
            onclick: move |_| on_close(()),
            div {
                class: "modal",
                onclick: move |e| e.stop_propagation(),
                h3 { "⚙ Configure pool: {pool_name}" }
                div { class: "tabs",
                    for t in [ConfigTab::General, ConfigTab::HealthCheck, ConfigTab::Advanced] {
                        button {
                            class: if *tab.read() == t { "active" } else { "" },
                            onclick: move |_| tab.set(t.clone()),
                            "{tab_name(&t)}"
                        }
                    }
                }
                match *tab.read() {
                    ConfigTab::General => rsx! {
                        div { class: "form-row",
                            div { class: "form-group",
                                label { "Algorithm" }
                                select {
                                    option { value: "round-robin", selected: algorithm == "round-robin", "Round Robin" }
                                    option { value: "least-conn", selected: algorithm == "least-conn", "Least Connections" }
                                    option { value: "source-ip", selected: algorithm == "source-ip", "Source IP Hash" }
                                }
                            }
                            div { class: "form-group",
                                label { "Max connections" }
                                input { r#type: "number", value: "{max_conns}", oninput: move |e| max_conns.set(e.value()) }
                            }
                        }
                    },
                    ConfigTab::HealthCheck => rsx! {
                        div { class: "form-row",
                            div { class: "form-group",
                                label { "Interval (s)" }
                                input { r#type: "number", value: "{hc_interval}", min: "1", max: "300", oninput: move |e| hc_interval.set(e.value()) }
                            }
                            div { class: "form-group",
                                label { "Timeout (s)" }
                                input { r#type: "number", value: "{hc_timeout}", min: "1", max: "30", oninput: move |e| hc_timeout.set(e.value()) }
                            }
                            div { class: "form-group",
                                label { "Retries" }
                                input { r#type: "number", value: "{hc_retries}", min: "1", max: "10", oninput: move |e| hc_retries.set(e.value()) }
                            }
                        }
                        div { class: "form-group",
                            label { "Health path (HTTP only)" }
                            input { value: "{hc_path}", placeholder: "/health", oninput: move |e| hc_path.set(e.value()) }
                        }
                    },
                    ConfigTab::Advanced => rsx! {
                        div { class: "form-row",
                            div { class: "form-group",
                                label { "Connection timeout (s)" }
                                input { r#type: "number", value: "{conn_timeout}", oninput: move |e| conn_timeout.set(e.value()) }
                            }
                        }
                        div { class: "form-group",
                            label { style: "display:flex;align-items:center;gap:8px;cursor:pointer",
                                input { r#type: "checkbox", checked: *drain_mode.read(),
                                    oninput: move |e| drain_mode.set(e.checked()) }
                                "Drain mode (stop accepting new connections)"
                            }
                        }
                    },
                }
                div { class: "modal-actions",
                    button { class: "btn", onclick: move |_| on_close(()), "Cancel" }
                    button { class: "btn", style: "background:var(--primary);color:#fff;border-color:var(--primary)",
                        onclick: move |_| on_close(()), "Apply" }
                }
            }
        }
    }
}

#[component]
fn LbView(lb: Signal<Result<LbData, String>>) -> Element {
    let mut config_pool: Signal<Option<String>> = use_signal(|| None);
    let body = match &*lb.read() {
        Ok(data) => {
            let mut tcp_rows = String::new();
            for p in &data.tcp {
                for b in &p.backends {
                    let pill_html = if b.healthy {
                        "<span class='pill ok'>healthy</span>"
                    } else {
                        "<span class='pill bad'>down</span>"
                    };
                    tcp_rows.push_str(&format!(
                        "<tr><td>{}</td><td>{}</td><td class='mono'>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                        p.name, p.algorithm, b.addr, pill_html,
                        b.active_conns, b.total_conns, p.active_conns, p.rejected_conns
                    ));
                }
            }
            let mut http_rows = String::new();
            for p in &data.http {
                for r in &p.routes {
                    let host = if r.host.is_empty() { "*".to_string() } else { r.host.clone() };
                    http_rows.push_str(&format!(
                        "<tr><td>{}</td><td>{}</td><td class='mono'>{}</td><td>{}</td></tr>",
                        p.name, host, r.path_prefix, r.backends
                    ));
                }
            }
            format!(
                "<h3>TCP Pools</h3><table><thead><tr><th>Pool</th><th>Algorithm</th><th>Backend</th><th>Health</th><th>Active</th><th>Total</th><th>Pool active</th><th>Rejected</th></tr></thead><tbody>{tcp_rows}</tbody></table><h3>HTTP Pools</h3><table><thead><tr><th>Pool</th><th>Host</th><th>Path prefix</th><th>Backends</th></tr></thead><tbody>{http_rows}</tbody></table>"
            )
        }
        Err(e) => format!("<div style='color:var(--red)'>{e}</div>"),
    };

    let (pool_names, selected) = {
        let cfg = config_pool.read();
        let names: Vec<String> = match &*lb.read() {
            Ok(d) => d.tcp.iter().map(|p| p.name.clone()).collect(),
            Err(_) => vec![],
        };
        let sel = cfg.as_ref().and_then(|n| {
            match &*lb.read() {
                Ok(d) => d.tcp.iter().find(|p| &p.name == n)
                    .map(|p| (p.name.clone(), p.algorithm.clone())),
                Err(_) => None,
            }
        });
        (names, sel)
    };

    rsx! {
        div {
            if !pool_names.is_empty() {
                div { class: "actions-bar", style: "margin-bottom:10px",
                    span { class: "lbl", style: "color:var(--muted);font-size:12px;margin-right:6px", "Deep config:" }
                    for name in pool_names.clone() {
                        button { class: "btn", onclick: move |_| config_pool.set(Some(name.clone())),
                            "⚙ {name}" }
                    }
                }
            }
            div { dangerous_inner_html: "{body}" }
            if let Some((pname, algo)) = &selected {
                PoolConfigModal {
                    key: "{pname}",
                    pool_name: pname.clone(),
                    algorithm: algo.clone(),
                    on_close: move |_| config_pool.set(None),
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Actions (customizable buttons)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ActionDef {
    label: String,
    method: String,
    url: String,
}

fn load_actions() -> Vec<ActionDef> {
    ls_get("sl-actions")
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_else(|| vec![
            ActionDef { label: "Reload config".into(), method: "POST".into(), url: "/api/reload".into() },
            ActionDef { label: "TLS reload".into(), method: "POST".into(), url: "/api/tls/reload".into() },
            ActionDef { label: "Generate keypair".into(), method: "GET".into(), url: "/api/mesh/keypair".into() },
        ])
}

fn save_actions(actions: &[ActionDef]) {
    if let Ok(json) = serde_json::to_string(actions) {
        ls_set("sl-actions", &json);
    }
}

#[component]
fn ActionsView() -> Element {
    let mut actions = use_signal(load_actions);
    let mut toast = use_signal(|| Option::<String>::None);
    let mut label = use_signal(|| String::new());
    let mut method = use_signal(|| "GET".to_string());
    let mut url = use_signal(|| String::new());

    let acts_snapshot = actions().clone();

    rsx! {
        {toast().map(|t| rsx! {
            div { class: "toast", "{t}" }
        })}

        div { class: "card",
            h3 { "Quick actions" }
            div { class: "actions-bar",
                for a in acts_snapshot.clone().into_iter() {
                    button { class: "act-btn",
                        onclick: move |_| {
                            let a = a.clone();
                            spawn(async move {
                                let msg = api::call(&a.method, &a.url).await;
                                let text = match msg { Ok(t) => t, Err(e) => e };
                                toast.set(Some(format!("{} → {}", a.label, text)));
                            });
                        },
                        span { "{a.label}" }
                    }
                }
                {if acts_snapshot.is_empty() { rsx! {
                    span { style: "color:var(--muted);font-size:13px", "no buttons — add one below" }
                }} else { rsx! {} }}
            }

            h3 { style: "margin-top:18px", "Customize — add a button" }
            div { class: "form-row",
                div { label { "Label" }
                    input { value: "{label}", placeholder: "Reload config", size: "16",
                        oninput: move |e| label.set(e.value()) }
                }
                div { label { "Method" }
                    select { value: "{method}",
                        oninput: move |e| method.set(e.value()),
                        option { value: "GET", "GET" }
                        option { value: "POST", "POST" }
                        option { value: "DELETE", "DELETE" }
                    }
                }
                div { label { "API path" }
                    input { value: "{url}", placeholder: "/api/reload", size: "24",
                        oninput: move |e| url.set(e.value()) }
                }
                button { class: "btn", onclick: move |_| {
                        if label().trim().is_empty() || url().trim().is_empty() {
                            toast.set(Some("label and URL are required".into()));
                            return;
                        }
                        let mut acts = actions();
                        acts.push(ActionDef {
                            label: label().trim().to_string(),
                            method: method(),
                            url: url().trim().to_string(),
                        });
                        save_actions(&acts);
                        actions.set(acts);
                        label.set(String::new());
                        url.set(String::new());
                    },
                    "+ Add button"
                }
            }

            h3 { style: "margin-top:18px", "Existing custom buttons" }
            {actions().iter().enumerate().map(|(i, a)| rsx! {
                div { class: "act-list-item", key: "{i}-{a.label}",
                    span {
                        b { "{a.label}" }
                        span { class: "pill info", "{a.method}" }
                        code { "{a.url}" }
                    }
                    button { class: "btn danger", onclick: move |_| {
                            let mut acts = actions();
                            acts.remove(i);
                            save_actions(&acts);
                            actions.set(acts);
                        },
                        "✕"
                    }
                }
            })}
        }
    }
}

// ---------------------------------------------------------------------------
// Topology (interactive: drag / zoom / pan, auto-layouts)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
struct TopoNode {
    id: String,
    label: String,
    kind: &'static str,
    sub: String,
    x: f64,
    y: f64,
}

#[derive(Clone, Debug)]
struct TopoEdge {
    from: String,
    to: String,
    label: String,
    color: String,
}

fn topo_color(kind: &str) -> &'static str {
    match kind {
        "hub" => "#00d2ff",
        "ok" => "#34d399",
        "down" => "#f87171",
        "http" => "#a78bfa",
        _ => "#8b98a5",
    }
}

#[component]
fn TopologyView(lb: Signal<Result<LbData, String>>) -> Element {
    let mut nodes: Signal<Vec<TopoNode>> = use_signal(Vec::new);
    let mut edges: Signal<Vec<TopoEdge>> = use_signal(Vec::new);
    let mut zoom = use_signal(|| 1.0f64);
    let mut pan_x = use_signal(|| 80.0f64);
    let mut pan_y = use_signal(|| 40.0f64);
    let mut drag: Signal<Option<(String, f64, f64)>> = use_signal(|| None);
    let mut panning = use_signal(|| false);
    let mut pan_start: Signal<(f64, f64)> = use_signal(|| (0.0, 0.0));

    use_effect(move || {
        let lb = lb.clone();
        spawn(async move {
            let data = match lb.read().clone() {
                Ok(d) => d,
                Err(_) => return,
            };
                let mut n = vec![TopoNode {
                    id: "hub".into(),
                    label: "sdwanlite".into(),
                    kind: "hub",
                    sub: String::new(),
                    x: 0.0,
                    y: 0.0,
                }];
                let mut e = Vec::new();
                for p in &data.tcp {
                    for b in &p.backends {
                        let id = format!("tcp:{}:{}", p.name, b.addr);
                        n.push(TopoNode {
                            id: id.clone(),
                            label: b.addr.clone(),
                            kind: if b.healthy { "ok" } else { "down" },
                            sub: p.name.clone(),
                            x: 0.0,
                            y: 0.0,
                        });
                        e.push(TopoEdge {
                            from: "hub".into(),
                            to: id,
                            label: p.name.clone(),
                            color: if b.healthy { "#34d399".into() } else { "#f87171".into() },
                        });
                    }
                }
                for p in &data.http {
                    for r in &p.routes {
                        let id = format!("http:{}:{}", p.name, r.path_prefix);
                        let label = format!(
                            "{}{}",
                            if r.host.is_empty() { "*".to_string() } else { r.host.clone() },
                            r.path_prefix
                        );
                        n.push(TopoNode {
                            id: id.clone(),
                            label,
                            kind: "http",
                            sub: p.name.clone(),
                            x: 0.0,
                            y: 0.0,
                        });
                        e.push(TopoEdge {
                            from: "hub".into(),
                            to: id,
                            label: p.name.clone(),
                            color: "#00d2ff".into(),
                        });
                    }
                }
                if let Some(saved) = ls_get("sl-topo") {
                    if let Ok(map) = serde_json::from_str::<std::collections::HashMap<String, (f64, f64)>>(&saved) {
                        for node in n.iter_mut() {
                            if let Some((x, y)) = map.get(&node.id) {
                                node.x = *x;
                                node.y = *y;
                            }
                        }
                    }
                }
                nodes.set(n);
                edges.set(e);
            }
        );
    });

    rsx! {
        div { class: "topo-toolbar",
            button { class: "btn", title: "Vertical layout", onclick: move |_| topo_apply_layout(&mut nodes, "vertical"), "⬍" }
            button { class: "btn", title: "Horizontal layout", onclick: move |_| topo_apply_layout(&mut nodes, "horizontal"), "⬌" }
            button { class: "btn", title: "Radial layout", onclick: move |_| topo_apply_layout(&mut nodes, "radial"), "◎" }
            span { class: "lbl", style: "margin-left:auto",
                "scroll = zoom · drag bg = pan · dbl-click = reset" }
        }
        svg {
            id: "topo-svg",
            ondoubleclick: move |_| { zoom.set(1.0); pan_x.set(80.0); pan_y.set(40.0); },
            xmlns: "http://www.w3.org/2000/svg",
            onwheel: move |e| {
                e.stop_propagation();
                let d = e.delta();
                let (dx, dy) = match d {
                    dioxus::html::geometry::WheelDelta::Pixels(p) => (p.x, p.y),
                    dioxus::html::geometry::WheelDelta::Lines(l) => (l.x, l.y),
                    dioxus::html::geometry::WheelDelta::Pages(pg) => (pg.x, pg.y),
                };
                let factor = if dy < 0.0 { 1.12 } else { 0.89 };
                zoom.set((zoom() * factor).clamp(0.35, 3.0));
            },
            onpointerdown: move |e| {
                let c = e.client_coordinates();
                pan_start.set((c.x, c.y));
                panning.set(true);
            },
            onpointermove: move |e| {
                if panning() {
                    let c = e.client_coordinates();
                    pan_x.set(pan_x() + c.x - pan_start().0);
                    pan_y.set(pan_y() + c.y - pan_start().1);
                }
            },
            onpointerup: move |_| {
                panning.set(false);
                if drag.read().is_some() {
                    let pos: std::collections::HashMap<String, (f64, f64)> = nodes
                        .read()
                        .iter()
                        .map(|n| (n.id.clone(), (n.x, n.y)))
                        .collect();
                    if let Ok(json) = serde_json::to_string(&pos) {
                        ls_set("sl-topo", &json);
                    }
                    drag.set(None);
                }
            },
            g {
                style: "transition: transform 0.15s ease-out",
                transform: "translate({pan_x()} {pan_y()}) scale({zoom()})",
                {edges().iter().map(|e| {
                    let (Some(a), Some(b)) = (
                        nodes().iter().find(|n| n.id == e.from).map(|n| (n.x, n.y)),
                        nodes().iter().find(|n| n.id == e.to).map(|n| (n.x, n.y)),
                    ) else { return rsx! {} };
                    let mx = (a.0 + b.0) / 2.0;
                    let my = (a.1 + b.1) / 2.0 - 8.0;
                    rsx! {
                        g { key: "{e.from}->{e.to}",
                            line { x1: "{a.0}", y1: "{a.1}", x2: "{b.0}", y2: "{b.1}",
                                stroke: "{e.color}", stroke_width: "1.6", opacity: "0.55" }
                            text { x: "{mx}", y: "{my}", text_anchor: "middle",
                                font_size: "10", fill: "var(--muted)", "{e.label}" }
                        }
                    }
                })}
                {nodes().iter().map(|n| {
                    let color = topo_color(n.kind);
                    let w = (n.label.len() as f64 * 7.5 + 24.0).max(90.0);
                    let nid = n.id.clone();
                    let sub = n.sub.clone();
                    let label = n.label.clone();
                    let x = n.x;
                    let y = n.y;
                    rsx! {
                        g { class: "topo-node", "data-node": "1", key: "{nid}",
                            transform: "translate({x} {y})",
                            onpointerdown: move |e| {
                                e.stop_propagation();
                                let c = e.client_coordinates();
                                drag.set(Some((nid.clone(), c.x, c.y)));
                            },
                            rect { x: "{-w / 2.0}", y: "-22", width: "{w}", height: "44",
                                rx: "10", fill: "var(--panel)", stroke: "{color}", stroke_width: "1.5" }
                            circle { cx: "{-w / 2.0 + 12.0}", cy: "0", r: "4", fill: "{color}" }
                            text { x: "6", y: "-2", font_size: "11.5", font_weight: "600",
                                fill: "var(--text)", "{label.chars().take(22).collect::<String>()}" }
                            {if !sub.is_empty() { rsx! {
                                text { x: "6", y: "12", font_size: "9.5", fill: "var(--muted)", "{sub}" }
                            }} else { rsx! {} }}
                        }
                    }
                })}
            }
        }
    }
}

fn topo_apply_layout(nodes: &mut Signal<Vec<TopoNode>>, mode: &str) {
    let mut list = nodes();
    let total = (list.len().saturating_sub(1)) as f64;
    let mut idx = 0i32;
    for n in list.iter_mut() {
        if n.id == "hub" {
            n.x = 0.0;
            n.y = 0.0;
            continue;
        }
        let i = idx as f64;
        idx += 1;
        match mode {
            "vertical" => {
                n.x = 0.0;
                n.y = (i - (total - 1.0) / 2.0) * 90.0;
            }
            "radial" => {
                let ang = (i / total.max(1.0)) * 2.0 * std::f64::consts::PI
                    - std::f64::consts::FRAC_PI_2;
                n.x = ang.cos() * 220.0;
                n.y = ang.sin() * 150.0;
            }
            _ => {
                n.x = (i - (total - 1.0) / 2.0) * 170.0;
                n.y = 0.0;
            }
        }
    }
    nodes.set(list);
    let pos: std::collections::HashMap<String, (f64, f64)> = nodes
        .read()
        .iter()
        .map(|n| (n.id.clone(), (n.x, n.y)))
        .collect();
    if let Ok(json) = serde_json::to_string(&pos) {
        ls_set("sl-topo", &json);
    }
}




// ---------------------------------------------------------------------------
// Firewall
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default, Deserialize)]
struct FirewallData {
    #[serde(default)]
    rules: Vec<FirewallRuleView>,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct FirewallRuleView {
    action: String,
    port: u16,
    protocol: String,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    comment: String,
}
#[component]
fn FirewallView() -> Element {
    let data = use_resource(|| async {
        let rsp = gloo_net::http::Request::get("/api/firewall").send().await.map_err(|e| e.to_string())?;
        rsp.json::<FirewallData>().await.map_err(|e| e.to_string())
    });
    let body = match data.read().as_ref() {
        Some(Ok(d)) if d.rules.is_empty() => "<div style='color:var(--muted);font-size:13px'>No rules. All traffic allowed.</div>".into(),
        Some(Ok(d)) => {
            let mut rows = String::new();
            for r in &d.rules {
                let ap = if r.action == "allow" { "<span class='pill ok'>allow</span>" } else { "<span class='pill bad'>deny</span>" };
                let src = r.source.clone().unwrap_or_else(|| "any".into());
                rows.push_str(&format!("<tr><td>{}</td><td class='mono'>{}</td><td>{}</td><td class='mono'>{}</td><td>{}</td></tr>", ap, r.port, r.protocol, src, r.comment));
            }
            format!("<table><thead><tr><th>Action</th><th>Port</th><th>Protocol</th><th>Source</th><th>Comment</th></tr></thead><tbody>{}</tbody></table>", rows)
        }
        Some(Err(e)) => format!("<div style='color:var(--red)'>Error: {e}</div>"),
        None => "<div style='color:var(--muted)'>loading…</div>".into(),
    };
    rsx! { div { class: "card", h3 { "Firewall Rules" } div { dangerous_inner_html: "{body}" } } }
}

#[derive(Clone, Debug, Default, Deserialize)]
struct AlertsData {
    #[serde(default)]
    count: usize,
    #[serde(default)]
    events: Vec<AlertEventView>,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct AlertEventView {
    timestamp: u64,
    severity: String,
    source: String,
    message: String,
}

fn chrono_fmt(ts: u64) -> String {
    let d = ts / 86400; let r = ts % 86400;
    format!("day+{d} {:02}:{:02}:{:02} UTC", r / 3600, (r % 3600) / 60, r % 60)
}

#[component]
fn AlertsView() -> Element {
    let data = use_resource(|| async {
        let rsp = gloo_net::http::Request::get("/api/alerts").send().await.map_err(|e| e.to_string())?;
        rsp.json::<AlertsData>().await.map_err(|e| e.to_string())
    });
    let body = match data.read().as_ref() {
        Some(Ok(d)) if d.events.is_empty() => "<div style='color:var(--muted);font-size:13px'>No events yet.</div>".into(),
        Some(Ok(d)) => {
            let mut rows = String::new();
            for e in d.events.iter().rev().take(50) {
                let (cls, pl) = match e.severity.as_str() {
                    "critical" => ("alert-critical", "<span class='pill bad'>critical</span>"),
                    "warn" => ("alert-warn", "<span class='pill warn'>warn</span>"),
                    _ => ("alert-info", "<span class='pill info'>info</span>"),
                };
                let ts = chrono_fmt(e.timestamp);
                rows.push_str(&format!("<div class='{cls}' style='padding:8px 0;border-bottom:1px solid var(--border);font-size:13px'>{pl} <span style='margin-left:8px'>{src}: {msg}</span> <span style='float:right;color:var(--muted);font-size:11px'>{ts}</span></div>", cls=cls, pl=pl, src=e.source, msg=e.message, ts=ts));
            }
            format!("<div>{}</div>", rows)
        }
        Some(Err(e)) => format!("<div style='color:var(--red)'>Error: {e}</div>"),
        None => "<div style='color:var(--muted)'>loading…</div>".into(),
    };
    rsx! { div { class: "card", h3 { "Event Log" } div { dangerous_inner_html: "{body}" } } }
}

#[component]
fn QosView() -> Element {
    let data = use_resource(|| async {
        let rsp = gloo_net::http::Request::get("/api/lb").send().await.map_err(|e| e.to_string())?;
        rsp.json::<LbData>().await.map_err(|e| e.to_string())
    });
    let body = match data.read().as_ref() {
        Some(Ok(d)) if d.tcp.is_empty() => "<div style='color:var(--muted);font-size:13px'>no pools</div>".into(),
        Some(Ok(d)) => {
            let mut rows = String::new();
            for p in &d.tcp {
                let healthy = p.backends.iter().filter(|b| b.healthy).count();
                let total = p.backends.len();
                let health = if healthy == total { "<span class='pill ok'>all healthy</span>".into() } else { format!("<span class='pill bad'>{}/{}</span>", healthy, total) };
                rows.push_str(&format!("<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>", p.name, p.algorithm, p.active_conns, p.rejected_conns, total, health));
            }
            format!("<table><thead><tr><th>Pool</th><th>Algorithm</th><th>Active</th><th>Rejected</th><th>Backends</th><th>Health</th></tr></thead><tbody>{}</tbody></table>", rows)
        }
        Some(Err(e)) => format!("<div style='color:var(--red)'>Error: {e}</div>"),
        None => "<div style='color:var(--muted)'>loading…</div>".into(),
    };
    rsx! { div { class: "card", h3 { "QoS Bandwidth Limits" } div { dangerous_inner_html: "{body}" } } }
}

fn main() {
    launch(app);
}
