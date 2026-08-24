//! sdwanlite-lb: Layer-4 TCP load balancer and HTTP/1.1 reverse proxy.

pub mod http;
pub use http::HttpLoadBalancer;

pub mod tcp;

use sdwanlite_core::Algorithm;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tokio::net::TcpStream;

#[derive(Debug)]
pub struct Backend {
    pub addr: SocketAddr,
    healthy: AtomicBool,
    active_conns: AtomicU64,
    total_conns: AtomicU64,
}

impl Backend {
    pub fn new(addr: SocketAddr) -> Arc<Self> {
        Arc::new(Self {
            addr,
            healthy: AtomicBool::new(true),
            active_conns: AtomicU64::new(0),
            total_conns: AtomicU64::new(0),
        })
    }

    pub fn is_healthy(&self) -> bool {
        self.healthy.load(Ordering::Relaxed)
    }

    pub fn set_healthy(&self, ok: bool) -> bool {
        self.healthy.swap(ok, Ordering::Relaxed) != ok
    }

    pub fn active_conns(&self) -> u64 {
        self.active_conns.load(Ordering::Relaxed)
    }

    pub fn total_conns(&self) -> u64 {
        self.total_conns.load(Ordering::Relaxed)
    }

    async fn connect(&self) -> std::io::Result<TcpStream> {
        let s = TcpStream::connect(self.addr).await?;
        self.active_conns.fetch_add(1, Ordering::Relaxed);
        self.total_conns.fetch_add(1, Ordering::Relaxed);
        Ok(s)
    }

    fn release(&self) {
        self.active_conns.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Select a backend from a list using the configured algorithm.
/// Returns None when no backend is currently healthy.
pub(crate) fn select_backend<'a>(
    backends: &'a [Arc<Backend>],
    algo: Algorithm,
    counter: &AtomicU64,
) -> Option<&'a Arc<Backend>> {
    let healthy: Vec<&Arc<Backend>> = backends.iter().filter(|b| b.is_healthy()).collect();
    if healthy.is_empty() {
        return None;
    }
    match algo {
        Algorithm::RoundRobin => {
            let i = (counter.fetch_add(1, Ordering::Relaxed) % healthy.len() as u64) as usize;
            Some(healthy[i])
        }
        Algorithm::LeastConnections => healthy.iter().copied().min_by_key(|b| b.active_conns()),
        Algorithm::Random => {
            use rand::Rng;
            let i = rand::thread_rng().gen_range(0..healthy.len());
            Some(healthy[i])
        }
    }
}

/// Spawn a periodic TCP health checker for the given backends.
pub(crate) fn spawn_health_checker(
    pool_name: String,
    backends: Vec<Arc<Backend>>,
    interval: std::time::Duration,
    timeout: std::time::Duration,
) {
    if backends.is_empty() || interval.is_zero() {
        return;
    }
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(interval).await;
            for be in &backends {
                let ok = tokio::time::timeout(timeout, TcpStream::connect(be.addr))
                    .await
                    .map(|r| r.is_ok())
                    .unwrap_or(false);
                if be.set_healthy(ok) {
                    tracing::info!(pool = %pool_name, backend = %be.addr, healthy = ok, "backend health changed");
                }
            }
        }
    });
}
