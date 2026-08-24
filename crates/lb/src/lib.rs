//! sdwanlite-lb: Layer-4 TCP load balancer and HTTP/1.1 reverse proxy.

pub mod http;
pub use http::HttpLoadBalancer;


mod tls;
pub use tls::load_tls_server_config;

/// Unified accepted connection: plain TCP or TLS-wrapped.
pub enum Conn {
    Plain(TcpStream),
    Tls(Box<tokio_rustls::server::TlsStream<TcpStream>>),
}

impl tokio::io::AsyncRead for Conn {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match &mut *self {
            Conn::Plain(s) => std::pin::Pin::new(s).poll_read(cx, buf),
            Conn::Tls(s) => std::pin::Pin::new(s.as_mut()).poll_read(cx, buf),
        }
    }
}

impl tokio::io::AsyncWrite for Conn {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        match &mut *self {
            Conn::Plain(s) => std::pin::Pin::new(s).poll_write(cx, buf),
            Conn::Tls(s) => std::pin::Pin::new(s.as_mut()).poll_write(cx, buf),
        }
    }
    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match &mut *self {
            Conn::Plain(s) => std::pin::Pin::new(s).poll_flush(cx),
            Conn::Tls(s) => std::pin::Pin::new(s.as_mut()).poll_flush(cx),
        }
    }
    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match &mut *self {
            Conn::Plain(s) => std::pin::Pin::new(s).poll_shutdown(cx),
            Conn::Tls(s) => std::pin::Pin::new(s.as_mut()).poll_shutdown(cx),
        }
    }
}

/// Cooperative stop flag shared with the serve loop.
#[derive(Default)]
pub struct StopFlag(pub std::sync::atomic::AtomicBool);

impl StopFlag {
    pub fn stop(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
    pub fn is_stopped(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

pub mod tcp;

use sdwanlite_core::Algorithm;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::atomic::Ordering;
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
