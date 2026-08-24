//! sdwanlite daemon: wires config, load balancers, mesh and BGP together.

mod server;

use anyhow::{Context, Result};
use sdwanlite_bgp::BgpSpeaker;
use sdwanlite_core::Config;
use std::sync::Arc;
use std::time::Instant;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("sdwanlited {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let config_path = args
        .get(1)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("sdwanlite.toml"));

    let (config, used_sample) = Config::load_or_sample(&config_path);
    if used_sample {
        tracing::warn!(path = %config_path.display(), "config not found or invalid, running with built-in sample");
    }
    let config = Arc::new(config);

    // Load balancers
    let mut tcp_pools = Vec::new();
    for pool in &config.lb.tcp_pools {
        tcp_pools.push(sdwanlite_lb::tcp::TcpLoadBalancer::bind(pool)
            .await
            .with_context(|| format!("binding tcp pool '{}'", pool.name))?);
    }
    let mut http_pools = Vec::new();
    for pool in &config.lb.http_pools {
        http_pools.push(sdwanlite_lb::HttpLoadBalancer::bind(pool)
            .await
            .with_context(|| format!("binding http pool '{}'", pool.name))?);
    }

    // BGP
    let bgp = if config.bgp.enabled {
        let speaker = BgpSpeaker::new(Arc::new(config.bgp.clone()));
        tokio::spawn({
            let speaker = speaker.clone();
            async move {
                if let Err(e) = speaker.run().await {
                    tracing::error!("bgp speaker stopped: {e}");
                }
            }
        });
        Some(speaker)
    } else {
        None
    };

    let state = Arc::new(server::AppState {
        config: config.clone(),
        started: Instant::now(),
        tcp_pools,
        http_pools,
        bgp,
    });

    for pool in &state.tcp_pools {
        let p = pool.clone();
        tokio::spawn(async move { p.serve().await });
    }
    for pool in &state.http_pools {
        let p = pool.clone();
        tokio::spawn(async move { p.serve().await });
    }
    // Optional: apply WireGuard mesh at boot
    if config.mesh.enabled && !config.mesh.private_key.is_empty() {
        match sdwanlite_mesh::apply(&config, std::path::Path::new("/etc/wireguard")).await {
            Ok(()) => tracing::info!("wireguard mesh applied"),
            Err(e) => tracing::warn!("mesh apply skipped: {e}"),
        }
    }

    // ACME issuance/renewal
    if config.acme.enabled {
        let cfg = config.acme.clone();
        tokio::spawn(async move {
            sdwanlite_acme::renew_loop(cfg).await;
        });
    }

    // API + dashboard
    let addr = format!("{}:{}", config.general.api_addr, config.general.api_port);
    let app = server::router(Arc::clone(&state));
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("api + dashboard listening on http://{addr}");

    // graceful shutdown: Ctrl-C stops accept loops, then in-flight connections
    // are given a moment before process exit
    let shutdown = tokio::signal::ctrl_c();
    tokio::select! {
        r = axum::serve(listener, app) => { r?; }
        _ = shutdown => {
            tracing::info!("shutdown signal received");
            for pool in &state.tcp_pools { pool.stop(); }
            for pool in &state.http_pools { pool.stop(); }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    }

    Ok(())
}
