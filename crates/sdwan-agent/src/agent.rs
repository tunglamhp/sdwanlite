//! On-device agent (P0).
//!
//! Lifecycle:
//!   1. Bootstrap with `--bootstrap-token xxx --device-id <uuid> --controller http://...`
//!   2. `register()` — POST `/api/v1/devices/register`
//!   3. `sync_loop()` — WebSocket to `/stream/config`, receives new configs
//!   4. `apply_config(new, verify_fn)` — snapshot → apply → verify → commit / rollback
//!   5. `get_telemetry()` — periodic POST `/api/v1/telemetry` every 10s
//!   6. `metrics()` — local Prometheus exposition for `sdwan-exporter` to scrape (P3)
//!
//! The agent does NOT install kernel state itself in P0 — `verify_fn` is the
//! single seam where the data-plane crates (`sdwanlite-mesh`, `sdwanlite-bgp`,
//! `sdwanlite-lb`, etc.) plug in. P1 wires the real verifiers.

use crate::error::{AgentError, Result};
use crate::telemetry::TelemetryFrame;
use arc_swap::ArcSwap;
use futures_util::{SinkExt, StreamExt};
use sdwan_core::{ConfigVersion, DeviceConfig};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use uuid::Uuid;

/// Verify callback contract.
///
/// Called after a tentative `apply` step but before commit. Returning `Ok(())`
/// authorises the agent to make the new config active and bump the version.
/// Returning `Err(msg)` triggers rollback to the previous config WITHOUT bumping
/// the version (see `Agent::apply_config`).
///
/// The callback is intentionally a boxed `Fn` so production code can hand the
/// agent a closure that talks to `sdwanlite-overlay`, `sdwanlite-routing`, etc.,
/// and tests can hand it a stub.
pub type VerifyFn = Box<dyn Fn(&DeviceConfig) -> std::result::Result<(), String> + Send + Sync>;

/// Configuration the agent reads at startup (CLI flags or env).
#[derive(Clone, Debug)]
pub struct AgentConfig {
    /// Controller base URL (RFC 5737 example: `http://127.0.0.1:8080`).
    pub controller_url: String,
    /// Bootstrap token. NEVER echoed back, NEVER logged (see AGENTS.md).
    pub bootstrap_token: String,
    /// Device identity (UUIDv4, generated once and persisted on-device).
    pub device_id: Uuid,
    /// Owning org (multi-tenant scope).
    pub org_id: Uuid,
    /// Site within the org.
    pub site_id: Uuid,
    /// Hostname reported at registration.
    pub hostname: String,
    /// Telemetry push cadence.
    pub telemetry_interval: Duration,
}

impl AgentConfig {
    /// Construct from CLI-style values. Validates that the controller URL parses
    /// (loopback-only by default; callers must explicitly opt into non-loopback).
    pub fn new(
        controller_url: impl Into<String>,
        bootstrap_token: impl Into<String>,
        device_id: Uuid,
        org_id: Uuid,
        site_id: Uuid,
        hostname: impl Into<String>,
    ) -> std::result::Result<Self, AgentError> {
        let url = controller_url.into();
        // RFC 5737 / loopback guard. Non-loopback requires an explicit flag in main.rs.
        if !(url.starts_with("http://127.0.0.1")
            || url.starts_with("http://localhost")
            || url.starts_with("http://[::1]"))
        {
            return Err(AgentError::Internal(format!(
                "controller_url must be loopback (got {url}) — pass --enable-live-actions to allow non-loopback"
            )));
        }
        Ok(Self {
            controller_url: url,
            bootstrap_token: bootstrap_token.into(),
            device_id,
            org_id,
            site_id,
            hostname: hostname.into(),
            telemetry_interval: Duration::from_secs(10),
        })
    }
}

/// Result of an `apply_config` call (used by tests + dashboards).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplyOutcome {
    pub new_version: ConfigVersion,
    pub verified: bool,
    /// The version that is active after this call (== new_version if verified,
    /// else unchanged).
    pub active_version: ConfigVersion,
    /// Human-readable error if `verified == false`.
    #[serde(default)]
    pub error: Option<String>,
}

/// The on-device agent.
pub struct Agent {
    cfg: AgentConfig,
    /// Hot-swap atomic of the current config. P1 watches this from the data plane.
    current: Arc<ArcSwap<DeviceConfig>>, // visible to tests in this crate; tests in tests/ use agent.current().store via pub fn below
    /// Pending verify fn installed by `apply_config`.
    verify: Mutex<Option<Arc<VerifyFn>>>,
}

impl Agent {
    /// Construct a new agent. The starting config is a placeholder with version 0
    /// — `register()` populates it.
    pub fn new(cfg: AgentConfig) -> std::result::Result<Self, AgentError> {
        let placeholder = DeviceConfig {
            device_id: cfg.device_id,
            org_id: cfg.org_id,
            site_id: cfg.site_id,
            hostname: cfg.hostname.clone(),
            interfaces: Vec::new(),
            tunnels: Vec::new(),
            routes: Vec::new(),
            firewall: sdwan_core::FirewallPolicy::default(),
            qos: sdwan_core::QosPolicy::default(),
            path_labels: Vec::new(),
            version: 0,
        };
        Ok(Self {
            cfg,
            current: Arc::new(ArcSwap::from_pointee(placeholder)),
            verify: Mutex::new(None),
        })
    }

    /// Current active config (cheap atomic load).
    pub fn current(&self) -> DeviceConfig {
        self.current.load_full().as_ref().clone()
    }

    /// Test/internal seam — set the active config directly. Production code uses
    /// ; this exists so integration tests can stage an initial
    /// config without going through register().
    #[doc(hidden)]
    pub fn set_current_for_test(&self, cfg: DeviceConfig) {
        self.current.store(Arc::new(cfg));
    }

    /// Install the verify callback. P1 wires a closure that talks to the data plane.
    pub async fn set_verify(&self, f: VerifyFn) {
        *self.verify.lock().await = Some(Arc::new(f));
    }

    /// POST `/api/v1/devices/register`. Idempotent: returns Ok even if already
    /// registered (the controller is the source of truth and returns 409 if so).
    pub async fn register(&self) -> Result<DeviceConfig> {
        let url = format!("{}/api/v1/devices/register", self.cfg.controller_url);
        let body = serde_json::json!({
            "device_id": self.cfg.device_id,
            "org_id": self.cfg.org_id,
            "site_id": self.cfg.site_id,
            "hostname": self.cfg.hostname,
            "version": 1,
        });
        // Use the reqwest-free http path? P0 keeps deps minimal — we hand-roll a
        // minimal POST via tokio-tungstenite + a future http upgrade. Simpler: depend
        // on reqwest (already in workspace for acme). Add to workspace deps.
        let resp = self.http_post_json(&url, &body).await?;
        // Pull the latest config from the controller — it may have a non-trivial
        // starting point the controller wants us to apply.
        let cfg_url = format!(
            "{}/api/v1/devices/{}/config",
            self.cfg.controller_url, self.cfg.device_id
        );
        let cfg: DeviceConfig = self.http_get_json(&cfg_url).await?;
        self.current.store(Arc::new(cfg.clone()));
        // Discard body of register — we already have what we need from /config.
        let _ = resp;
        Ok(cfg)
    }

    /// Transactional apply: snapshot → verify → commit / rollback.
    ///
    /// Contract (P0):
    ///   1. **Snapshot** — clone `current`.
    ///   2. **Refuse** if `new.version <= snapshot.version` (optimistic locking).
    ///   3. **Verify** — call the registered `verify_fn(&new)`. On `Err`, return
    ///      `ApplyOutcome { verified: false, active_version: snapshot.version,
    ///      error: Some(msg) }` WITHOUT touching the swap. The previous config
    ///      remains active.
    ///   4. **Commit** — bump `new.version` and store via `ArcSwap::store`. Return
    ///      `ApplyOutcome { verified: true, active_version: new.version, .. }`.
    pub async fn apply_config(&self, new: DeviceConfig) -> ApplyOutcome {
        let snapshot = self.current();
        if !new.is_strictly_newer_than(&snapshot) {
            return ApplyOutcome {
                new_version: new.version,
                verified: false,
                active_version: snapshot.version,
                error: Some(format!(
                    "stale: incoming v{} <= active v{}",
                    new.version, snapshot.version
                )),
            };
        }
        // Verify step.
        let verify = self.verify.lock().await.clone();
        match verify {
            None => {
                // No verifier installed → for P0 we accept the apply unconditionally
                // (the caller is responsible for verifying externally). This makes
                // the data plane pluggable.
                let committed = new.clone().with_bumped_version();
                self.current.store(Arc::new(committed.clone()));
                ApplyOutcome {
                    new_version: committed.version,
                    verified: true,
                    active_version: committed.version,
                    error: None,
                }
            }
            Some(f) => match f(&new) {
                Ok(()) => {
                    let committed = new.clone().with_bumped_version();
                    self.current.store(Arc::new(committed.clone()));
                    ApplyOutcome {
                        new_version: committed.version,
                        verified: true,
                        active_version: committed.version,
                        error: None,
                    }
                }
                Err(msg) => {
                    // Rollback: do NOT touch `self.current`. The snapshot is still live.
                    tracing::warn!(error = %msg, version = new.version, "verify failed; rolling back");
                    ApplyOutcome {
                        new_version: new.version,
                        verified: false,
                        active_version: snapshot.version,
                        error: Some(msg),
                    }
                }
            },
        }
    }

    /// WebSocket sync loop. Connects to `<controller>/stream/config` and pipes
    /// each delta through `apply_config`. Reconnects with exponential backoff on
    /// transport failure.
    pub async fn sync_loop(&self) {
        let ws_url = ws_url(&self.cfg.controller_url);
        let mut backoff = Duration::from_millis(500);
        loop {
            match self.ws_connect_and_drain(&ws_url).await {
                Ok(()) => {
                    backoff = Duration::from_millis(500);
                }
                Err(e) => {
                    tracing::warn!(error = %e, "ws sync failed; backing off");
                    tokio::time::sleep(backoff).await;
                    backoff = (backoff * 2).min(Duration::from_secs(30));
                }
            }
        }
    }

    async fn ws_connect_and_drain(&self, url: &str) -> Result<()> {
        let (mut ws, _resp) = tokio_tungstenite::connect_async(url)
            .await
            .map_err(|e| AgentError::Websocket(e.to_string()))?;
        // Inject the bearer token as the first text frame — the controller's WS
        // upgrade requires it (see `check_auth` in controller.rs).
        ws.send(tokio_tungstenite::tungstenite::protocol::Message::Text(
            format!("Bearer {}", self.cfg.bootstrap_token),
        ))
        .await
        .map_err(|e| AgentError::Websocket(e.to_string()))?;

        while let Some(msg) = ws.next().await {
            match msg {
                Ok(tokio_tungstenite::tungstenite::protocol::Message::Text(t)) => {
                    match serde_json::from_str::<DeviceConfig>(&t) {
                        Ok(cfg) => {
                            let outcome = self.apply_config(cfg).await;
                            tracing::info!(
                                verified = outcome.verified,
                                active_version = outcome.active_version,
                                "sync_loop apply"
                            );
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "ws decode");
                        }
                    }
                }
                Ok(tokio_tungstenite::tungstenite::protocol::Message::Close(_)) => break,
                Err(e) => return Err(AgentError::Websocket(e.to_string())),
                _ => {}
            }
        }
        Ok(())
    }

    /// POST telemetry frame to the controller.
    pub async fn get_telemetry(&self) -> Result<()> {
        let url = format!("{}/api/v1/telemetry", self.cfg.controller_url);
        let frame = TelemetryFrame {
            device_id: self.cfg.device_id,
            org_id: self.cfg.org_id,
            uptime_secs: 0, // P0 stub
            links: Vec::new(),
            flags: Vec::new(),
        };
        let _ = self.http_post_json(&url, &frame).await?;
        Ok(())
    }

    /// Prometheus exposition. P3 scrapes this from `sdwan-exporter`.
    pub fn metrics(&self) -> String {
        let c = self.current();
        format!(
            "# HELP sdwan_agent_config_version Active config version.\n\
             # TYPE sdwan_agent_config_version gauge\n\
             sdwan_agent_config_version {{device_id=\"{}\"}} {}\n\
             # HELP sdwan_agent_up Agent liveness.\n\
             # TYPE sdwan_agent_up gauge\n\
             sdwan_agent_up{{device_id=\"{}\"}} 1\n",
            c.device_id, c.version, c.device_id
        )
    }

    // ---- private HTTP helpers ----

    async fn http_post_json<T: Serialize>(&self, url: &str, body: &T) -> Result<serde_json::Value> {
        use tokio::net::TcpStream;
        let req = format!(
            "POST {path} HTTP/1.1\r\nHost: {host}\r\nAuthorization: Bearer {tok}\r\nContent-Type: application/json\r\nContent-Length: {len}\r\nConnection: close\r\n\r\n{payload}",
            path = url_path(url),
            host = url_host(url),
            tok = self.cfg.bootstrap_token,
            len = serde_json::to_string(body).unwrap_or_default().len(),
            payload = serde_json::to_string(body).unwrap_or_default(),
        );
        // Hand-rolled HTTP/1.1 over a TCP stream — keeps the dep surface tiny for P0.
        let stream = TcpStream::connect(url_addr(url)).await.map_err(http)?;
        let (mut r, mut w) = tokio::io::split(stream);
        use tokio::io::AsyncWriteExt;
        w.write_all(req.as_bytes()).await.map_err(http)?;
        w.shutdown().await.map_err(http)?;
        let mut buf = Vec::new();
        use tokio::io::AsyncReadExt;
        r.read_to_end(&mut buf).await.map_err(http)?;
        let text = String::from_utf8_lossy(&buf).into_owned();
        let body = text.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
        if body.is_empty() {
            Ok(serde_json::json!({}))
        } else {
            serde_json::from_str(&body).map_err(|e| AgentError::Http(e.to_string()))
        }
    }

    async fn http_get_json<T: for<'de> Deserialize<'de>>(&self, url: &str) -> Result<T> {
        use tokio::net::TcpStream;
        let req = format!(
            "GET {path} HTTP/1.1\r\nHost: {host}\r\nAuthorization: Bearer {tok}\r\nConnection: close\r\n\r\n",
            path = url_path(url),
            host = url_host(url),
            tok = self.cfg.bootstrap_token,
        );
        let stream = TcpStream::connect(url_addr(url)).await.map_err(http)?;
        let (mut r, mut w) = tokio::io::split(stream);
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        w.write_all(req.as_bytes()).await.map_err(http)?;
        w.shutdown().await.map_err(http)?;
        let mut buf = Vec::new();
        r.read_to_end(&mut buf).await.map_err(http)?;
        let text = String::from_utf8_lossy(&buf).into_owned();
        let body = text.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
        serde_json::from_str(&body).map_err(|e| AgentError::Http(e.to_string()))
    }
}

fn http(e: std::io::Error) -> AgentError {
    AgentError::Http(e.to_string())
}

/// Translate `http://host:port/path` → `host:port`.
fn url_addr(url: &str) -> &str {
    url.splitn(4, '/').nth(3).unwrap_or("127.0.0.1:8080")
}
/// Translate `http://host:port/path` → `/path` (RFC 3986 path).
fn url_path(url: &str) -> &str {
    let i = url.find("://").map(|i| i + 3).unwrap_or(0);
    url[i..]
        .split_once('/')
        .map(|x| x.1)
        .map(|p| format!("/{p}"))
        .unwrap_or_else(|| "/".into())
        .leak() as &str // P0: OK for short-lived requests
}
/// Translate `http://host:port/path` → `host` (no port).
fn url_host(url: &str) -> &str {
    let i = url.find("://").map(|i| i + 3).unwrap_or(0);
    url[i..].split(':').next().unwrap_or("127.0.0.1")
}
/// Translate `http://host:port/path` → `ws://host:port/stream/config`.
fn ws_url(http_url: &str) -> String {
    let i = http_url.find("://").map(|i| i + 3).unwrap_or(0);
    let rest = &http_url[i..];
    format!("ws://{rest}/stream/config")
}
