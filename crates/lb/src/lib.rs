//! sdwanlite-lb: Layer-4 TCP load balancer and HTTP/1.1 reverse proxy.

pub mod http;
pub use http::HttpLoadBalancer;

mod h2up;
pub mod tls;
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
use std::sync::atomic::Ordering;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize};
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use tokio::net::TcpStream;

#[derive(Debug)]
pub struct Backend {
    pub addr: SocketAddr,
    healthy: AtomicBool,
    active_conns: AtomicU64,
    total_conns: AtomicU64,
    rx_bytes: AtomicU64,
    tx_bytes: AtomicU64,
    health_failures: AtomicU64,
    /// Last 64 connect latencies in microseconds (ring buffer, 0 = empty slot).
    latencies: Mutex<[u64; 64]>,
    latencies_next: AtomicUsize,
}

impl Backend {
    pub fn new(addr: SocketAddr) -> Arc<Self> {
        Arc::new(Self {
            addr,
            healthy: AtomicBool::new(true),
            active_conns: AtomicU64::new(0),
            total_conns: AtomicU64::new(0),
            rx_bytes: AtomicU64::new(0),
            tx_bytes: AtomicU64::new(0),
            health_failures: AtomicU64::new(0),
            latencies: Mutex::new([0; 64]),
            latencies_next: AtomicUsize::new(0),
        })
    }

    pub fn is_healthy(&self) -> bool {
        self.healthy.load(Ordering::Relaxed)
    }

    pub fn set_healthy(&self, ok: bool) -> bool {
        if !ok {
            self.health_failures.fetch_add(1, Ordering::Relaxed);
        }
        self.healthy.swap(ok, Ordering::Relaxed) != ok
    }

    pub fn active_conns(&self) -> u64 {
        self.active_conns.load(Ordering::Relaxed)
    }

    pub fn total_conns(&self) -> u64 {
        self.total_conns.load(Ordering::Relaxed)
    }

    pub fn rx_bytes(&self) -> u64 {
        self.rx_bytes.load(Ordering::Relaxed)
    }

    pub fn tx_bytes(&self) -> u64 {
        self.tx_bytes.load(Ordering::Relaxed)
    }

    pub fn health_failures(&self) -> u64 {
        self.health_failures.load(Ordering::Relaxed)
    }

    pub fn bump_health_failures(&self) {
        self.health_failures.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_latency_us(&self, us: u64) {
        let idx = self.latencies_next.fetch_add(1, Ordering::Relaxed) % 64;
        if let Ok(mut buf) = self.latencies.lock() {
            buf[idx] = us;
        }
    }

    /// Latency percentiles in microseconds from the ring buffer (no samples -> None).
    pub fn latency_percentiles_us(&self) -> (Option<u64>, Option<u64>) {
        let buf = match self.latencies.lock() {
            Ok(b) => *b,
            Err(_) => return (None, None),
        };
        let mut v: Vec<u64> = buf.iter().copied().filter(|x| *x > 0).collect();
        if v.is_empty() {
            return (None, None);
        }
        v.sort_unstable();
        let p = |q: usize| -> u64 { v[(v.len() * q).div_ceil(100).max(1) - 1] };
        (Some(p(50)), Some(p(95)))
    }

    pub(crate) fn add_bytes(&self, up: u64, down: u64) {
        // "up" = client->backend, "down" = backend->client
        if up > 0 {
            self.tx_bytes.fetch_add(up, Ordering::Relaxed);
        }
        if down > 0 {
            self.rx_bytes.fetch_add(down, Ordering::Relaxed);
        }
    }

    async fn connect(&self) -> std::io::Result<TcpStream> {
        const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
        let started = std::time::Instant::now();
        let s = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(self.addr))
            .await
            .map_err(|_| {
                self.record_latency_us(CONNECT_TIMEOUT.as_micros() as u64);
                std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!("connect timeout to {}", self.addr),
                )
            })??;
        self.record_latency_us(started.elapsed().as_micros() as u64);
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
        // Failover: always prefer the first backend in the list that is healthy
        Algorithm::Failover => healthy.first().copied(),
    }
}

/// Health check mode.
#[derive(Clone, Debug)]
pub enum HealthCheck {
    /// Bare TCP connect.
    Tcp,
    /// HTTP GET to `path`; 2xx/3xx status counts as healthy.
    Http(String),
}

impl HealthCheck {
    pub fn from_path(path: Option<&String>) -> Self {
        match path {
            Some(p) => HealthCheck::Http(p.clone()),
            None => HealthCheck::Tcp,
        }
    }
}

async fn probe(be: &Backend, mode: &HealthCheck, timeout: Duration) -> bool {
    let attempt = async {
        let mut sock = TcpStream::connect(be.addr).await?;
        match mode {
            HealthCheck::Tcp => Ok(true),
            HealthCheck::Http(path) => {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let req = format!(
                    "GET {path} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
                    be.addr
                );
                sock.write_all(req.as_bytes()).await?;
                let mut buf = [0u8; 512];
                let n = sock.read(&mut buf).await.unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]);
                // "HTTP/1.1 200 OK" -> parse the status code token
                let ok = head
                    .split_whitespace()
                    .nth(1)
                    .and_then(|c| c.parse::<u16>().ok())
                    .map(|code| (200..400).contains(&code))
                    .unwrap_or(false);
                Ok::<bool, std::io::Error>(ok)
            }
        }
    };
    tokio::time::timeout(timeout, attempt)
        .await
        .unwrap_or(Ok(false))
        .unwrap_or(false)
}

/// Spawn a periodic health checker for the given backends.
pub(crate) fn spawn_health_checker(
    pool_name: String,
    backends: Vec<Arc<Backend>>,
    interval: std::time::Duration,
    timeout: std::time::Duration,
    mode: HealthCheck,
) {
    if backends.is_empty() || interval.is_zero() {
        return;
    }
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(interval).await;
            for be in &backends {
                let ok = probe(be, &mode, timeout).await;
                if be.set_healthy(ok) {
                    tracing::info!(pool = %pool_name, backend = %be.addr, healthy = ok, "backend health changed");
                }
            }
        }
    });
}

// ---------------------------------------------------------------------------
// Firewall enforcement
// ---------------------------------------------------------------------------

/// Check if a connection from `source_ip` on `port` is allowed by the firewall rules.
/// Rules are evaluated in order; first match wins. No match = allow.
pub fn firewall_check(
    rules: &[sdwanlite_core::FirewallRule],
    source_ip: &str,
    port: u16,
    protocol: &str,
) -> bool {
    for rule in rules {
        if rule.port != port && rule.port != 0 {
            continue;
        }
        if rule.protocol != "any" && rule.protocol != protocol {
            continue;
        }
        if let Some(src) = &rule.source {
            if !src.is_empty() && !source_ip.starts_with(src.trim_end_matches('*')) {
                continue;
            }
        }
        return rule.action == "allow";
    }
    true // default allow
}

// ---------------------------------------------------------------------------
// Alert event log (ring buffer)
// ---------------------------------------------------------------------------

pub struct AlertLog {
    events: Mutex<Vec<sdwanlite_core::AlertEvent>>,
    max: usize,
}

impl AlertLog {
    pub fn new(max: usize) -> Self {
        Self {
            events: Mutex::new(Vec::new()),
            max,
        }
    }

    pub fn push(&self, severity: &str, source: &str, message: &str) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let mut events = self.events.lock().unwrap();
        events.push(sdwanlite_core::AlertEvent {
            timestamp: now,
            severity: severity.into(),
            source: source.into(),
            message: message.into(),
        });
        if events.len() > self.max {
            events.remove(0);
        }
    }

    pub fn list(&self) -> Vec<sdwanlite_core::AlertEvent> {
        self.events.lock().unwrap().clone()
    }
}

#[cfg(test)]
mod golden_signals_tests {
    use super::*;

    #[test]
    fn latency_percentiles_empty() {
        let b = Backend::new("127.0.0.1:80".parse().unwrap());
        assert_eq!(b.latency_percentiles_us(), (None, None));
    }

    #[test]
    fn latency_percentiles_p50_p95() {
        let b = Backend::new("127.0.0.1:80".parse().unwrap());
        for us in [100u64, 200, 300, 400, 500, 600, 700, 800, 900, 1000] {
            b.record_latency_us(us);
        }
        let (p50, p95) = b.latency_percentiles_us();
        assert_eq!(p50, Some(500));
        assert_eq!(p95, Some(1000));
    }

    #[test]
    fn latency_ring_overwrites() {
        let b = Backend::new("127.0.0.1:80".parse().unwrap());
        for i in 0..70u64 {
            b.record_latency_us(i + 1);
        }
        // only the last 64 samples remain (7..=70); p50 of that set
        let (p50, _) = b.latency_percentiles_us();
        assert!(p50.unwrap() >= 7 && p50.unwrap() <= 70);
    }

    #[test]
    fn health_failures_counted() {
        let b = Backend::new("127.0.0.1:80".parse().unwrap());
        b.set_healthy(false);
        b.set_healthy(true);
        b.set_healthy(false);
        assert_eq!(b.health_failures(), 2);
    }
}
