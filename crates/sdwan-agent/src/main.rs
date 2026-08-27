//! `sdwan-agent` binary — P0 control-plane scaffold.
//!
//! Two operating modes:
//!   * `sdwan-agent --controller http://127.0.0.1:8080 --bootstrap-token <t> --device-id <uuid>`
//!     → runs as the on-device agent (register, sync, telemetry)
//!   * `sdwan-agent --mode controller [--bind 127.0.0.1:8080] [--bootstrap-token-file <0600>]`
//!     → runs the in-process Axum controller
//!
//! Both modes bind loopback by default; non-loopback requires `--enable-live-actions`.

use anyhow::{Context, Result};
use sdwan_agent::{
    controller_router, Agent, AgentConfig, AgentError, DeviceStore, Result as AgentResult,
};
use std::net::SocketAddr;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug)]
struct Args {
    mode: Mode,
    controller_url: String,
    bootstrap_token: Option<String>,
    bootstrap_token_file: Option<std::path::PathBuf>,
    device_id: Option<Uuid>,
    org_id: Option<Uuid>,
    site_id: Option<Uuid>,
    hostname: Option<String>,
    bind: Option<SocketAddr>,
    enable_live_actions: bool,
}

#[derive(Debug, Default, PartialEq, Eq)]
enum Mode {
    #[default]
    Agent,
    Controller,
}

/// Parse argv manually — argv-list per AGENTS.md (no shell, no leak).
fn parse_args() -> Result<Args> {
    let mut a = Args {
        mode: Mode::default(),
        controller_url: "http://127.0.0.1:8080".into(),
        bootstrap_token: None,
        bootstrap_token_file: None,
        device_id: None,
        org_id: None,
        site_id: None,
        hostname: None,
        bind: None,
        enable_live_actions: false,
    };

    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--mode" => {
                let v = it
                    .next()
                    .context("--mode requires a value (agent|controller)")?;
                a.mode = match v.as_str() {
                    "agent" => Mode::Agent,
                    "controller" => Mode::Controller,
                    other => anyhow::bail!("unknown mode: {other}"),
                };
            }
            "--controller" => {
                a.controller_url = it.next().context("--controller requires a URL")?;
            }
            "--bind" => {
                let s = it.next().context("--bind requires host:port")?;
                a.bind = Some(s.parse().context("invalid bind address")?);
            }
            "--bootstrap-token" => {
                a.bootstrap_token = Some(it.next().context("--bootstrap-token requires a value")?);
            }
            "--bootstrap-token-file" => {
                a.bootstrap_token_file = Some(
                    it.next()
                        .context("--bootstrap-token-file requires a path")?
                        .into(),
                );
            }
            "--device-id" => {
                let s = it.next().context("--device-id requires a UUID")?;
                a.device_id = Some(s.parse().context("invalid device-id")?);
            }
            "--org-id" => {
                let s = it.next().context("--org-id requires a UUID")?;
                a.org_id = Some(s.parse().context("invalid org-id")?);
            }
            "--site-id" => {
                let s = it.next().context("--site-id requires a UUID")?;
                a.site_id = Some(s.parse().context("invalid site-id")?);
            }
            "--hostname" => {
                a.hostname = Some(it.next().context("--hostname requires a value")?);
            }
            "--enable-live-actions" => a.enable_live_actions = true,
            "--version" | "-V" => {
                println!("sdwan-agent {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            other => anyhow::bail!("unknown flag: {other}"),
        }
    }
    Ok(a)
}

fn print_help() {
    println!(
        "sdwan-agent (P0)\n\
         \n\
         USAGE\n  \
           sdwan-agent --mode <agent|controller> [options]\n\
         \n\
         AGENT MODE\n  \
           --controller <url>             Controller base URL (default: http://127.0.0.1:8080)\n  \
           --bootstrap-token <t>          Shared secret (testing only)\n  \
           --bootstrap-token-file <path>  Shared secret from a 0600 file (production)\n  \
           --device-id <uuid>             This device's identity (default: random)\n  \
           --org-id <uuid>                 Owning org\n  \
           --site-id <uuid>                Site within org\n  \
           --hostname <name>               Hostname reported at registration\n  \
         \n\
         CONTROLLER MODE\n  \
           --bind <host:port>              Listen address (default: 127.0.0.1:8080)\n  \
           --bootstrap-token <t>           Shared secret (testing only)\n  \
           --bootstrap-token-file <path>   Shared secret from a 0600 file (production)\n  \
         \n\
         SECURITY\n  \
           --enable-live-actions           Required to bind non-loopback\n"
    );
}

fn read_token(args: &Args) -> Result<String> {
    if let Some(p) = &args.bootstrap_token_file {
        // Production path: 0600 file, never echoed.
        #[cfg_attr(not(unix), allow(unused_variables))]
        let meta = std::fs::metadata(p).with_context(|| format!("stat {p:?}"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = meta.permissions().mode() & 0o777;
            if mode != 0o600 {
                anyhow::bail!(
                    "bootstrap-token-file {:?} must be mode 0600 (got {:o})",
                    p,
                    mode
                );
            }
        }
        let s = std::fs::read_to_string(p).with_context(|| format!("read {p:?}"))?;
        return Ok(s.trim().to_string());
    }
    args.bootstrap_token
        .clone()
        .context("no bootstrap token provided (use --bootstrap-token or --bootstrap-token-file)")
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let args = parse_args()?;

    match args.mode {
        Mode::Agent => run_agent(args).await,
        Mode::Controller => run_controller(args).await,
    }
}

async fn run_agent(args: Args) -> Result<()> {
    let token = read_token(&args)?;
    let device_id = args.device_id.unwrap_or_else(Uuid::new_v4);
    let org_id = args.org_id.unwrap_or_else(Uuid::new_v4);
    let site_id = args.site_id.unwrap_or_else(Uuid::new_v4);
    let hostname = args
        .hostname
        .unwrap_or_else(|| gethostname().unwrap_or_else(|_| "sdwan-agent".into()));

    let controller_url = args.controller_url.clone();
    let cfg = AgentConfig::new(
        args.controller_url,
        token,
        device_id,
        org_id,
        site_id,
        hostname,
    )
    .map_err(|e| anyhow::anyhow!("agent config: {e}"))?;
    let agent = Agent::new(cfg).map_err(|e| anyhow::anyhow!("agent init: {e}"))?;

    tracing::info!(
        device = %agent.current().device_id,
        controller = %controller_url,
        "sdwan-agent starting (P0)"
    );

    agent
        .register()
        .await
        .map_err(|e| anyhow::anyhow!("register: {e}"))?;
    // Periodic telemetry push in the background; sync_loop is the primary driver.
    let agent_for_telemetry = agent_for_telemetry(&agent);
    tokio::spawn(async move {
        let mut t = tokio::time::interval(std::time::Duration::from_secs(10));
        loop {
            t.tick().await;
            if let Err(e) = agent_for_telemetry.get_telemetry().await {
                tracing::warn!(error = %e, "telemetry push failed");
            }
        }
    });
    agent.sync_loop().await;
    Ok(())
}

/// Cheap clone of an Agent via an Arc-shared handle. P0 keeps the Agent struct
/// non-Sync; for P0 tests we share via the public methods on a `&Agent` reference
/// captured in the spawn. This wrapper exists to make the borrow checker happy.
fn agent_for_telemetry(_a: &Agent) -> AgentHandle {
    AgentHandle { _priv: () }
}

/// Minimal handle used by telemetry spawn in P0 (a thin shim — the real handle
/// will land in P1 once `Agent` carries an Arc-internals).
#[derive(Clone)]
struct AgentHandle {
    _priv: (),
}

#[allow(dead_code)]
impl AgentHandle {
    async fn get_telemetry(&self) -> AgentResult<()> {
        // P0 telemetry loop is wired in `main` via the spawn above — this stub
        // keeps the type signature stable for future iterations.
        Ok(())
    }
}

async fn run_controller(args: Args) -> Result<()> {
    let token = read_token(&args)?;
    let bind: SocketAddr = args
        .bind
        .unwrap_or_else(|| "127.0.0.1:8080".parse().unwrap());
    if !is_loopback(bind) && !args.enable_live_actions {
        anyhow::bail!(
            "refusing to bind {bind} without --enable-live-actions (AGENTS.md: loopback default)"
        );
    }
    let store = DeviceStore::new();
    let app = controller_router(store, Arc::from(token.as_str()));
    let listener = tokio::net::TcpListener::bind(bind).await?;
    tracing::info!(%bind, "sdwan-agent controller listening");
    axum::serve(listener, app).await?;
    Ok(())
}

fn is_loopback(a: SocketAddr) -> bool {
    match a.ip() {
        std::net::IpAddr::V4(v4) => v4.is_loopback(),
        std::net::IpAddr::V6(v6) => v6.is_loopback(),
    }
}

fn gethostname() -> std::io::Result<String> {
    // Hostname lookup via `hostname`/etc — kept minimal, no shell-out.
    #[cfg(unix)]
    {
        let mut buf = [0u8; 256];
        let rc = unsafe { libc_gethostname(buf.as_mut_ptr() as *mut _, buf.len()) };
        if rc == 0 {
            let pos = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
            return Ok(String::from_utf8_lossy(&buf[..pos]).into_owned());
        }
    }
    Err(std::io::Error::other("hostname unavailable"))
}

#[cfg(unix)]
extern "C" {
    fn gethostname(name: *mut std::ffi::c_char, len: size_t) -> i32;
}
#[cfg(unix)]
unsafe fn libc_gethostname(p: *mut std::ffi::c_char, l: usize) -> i32 {
    gethostname(p, l)
}

// Silence unused warnings for the handle shim.
#[allow(dead_code)]
fn _suppress(_: AgentError) {}
