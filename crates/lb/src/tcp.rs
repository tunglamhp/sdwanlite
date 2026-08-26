//! Layer-4 TCP load balancer with transparent bidirectional forwarding.

use crate::{select_backend, spawn_health_checker, Backend, HealthCheck, StopFlag};
use sdwanlite_core::{Algorithm, TcpPool};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{copy_bidirectional, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::RwLock;

pub struct TcpLoadBalancer {
    pub name: String,
    listener: TcpListener,
    algo: Algorithm,
    backends: RwLock<Vec<Arc<Backend>>>,
    counter: AtomicU64,
    stop: Arc<StopFlag>,
    max_conns: AtomicUsize,
    active_conns: AtomicUsize,
    rejected_conns: AtomicU64,
}

impl TcpLoadBalancer {
    pub async fn bind(pool: &TcpPool) -> std::io::Result<Arc<Self>> {
        let listener = TcpListener::bind(&pool.listen).await?;
        let backends: Vec<Arc<Backend>> = pool
            .backends
            .iter()
            .filter_map(|s| s.parse().ok().map(Backend::new))
            .collect();
        let lb = Arc::new(Self {
            name: pool.name.clone(),
            listener,
            algo: pool.algorithm,
            backends: RwLock::new(backends),
            counter: AtomicU64::new(0),
            stop: Arc::new(StopFlag::default()),
            max_conns: AtomicUsize::new(0), // 0 = unlimited
            active_conns: AtomicUsize::new(0),
            rejected_conns: AtomicU64::new(0),
        });
        let hc_backends = lb.backends.read().await.clone();
        spawn_health_checker(
            pool.name.clone(),
            hc_backends,
            Duration::from_secs(pool.health_interval_secs),
            Duration::from_secs(pool.health_timeout_secs),
            HealthCheck::from_path(pool.health_check_path.as_ref()),
        );
        tracing::info!(pool = %pool.name, listen = %pool.listen, "tcp load balancer listening");
        Ok(lb)
    }

    /// Snapshot of current backends.
    pub async fn backends(&self) -> Vec<Arc<Backend>> {
        self.backends.read().await.clone()
    }

    /// Add a backend at runtime. Returns false if it already exists.
    pub async fn add_backend(&self, addr: std::net::SocketAddr) -> bool {
        let mut b = self.backends.write().await;
        if b.iter().any(|x| x.addr == addr) {
            return false;
        }
        b.push(Backend::new(addr));
        true
    }

    /// Remove a backend at runtime by address.
    pub async fn remove_backend(&self, addr: std::net::SocketAddr) -> bool {
        let mut b = self.backends.write().await;
        let before = b.len();
        b.retain(|x| x.addr != addr);
        b.len() != before
    }

    pub fn algorithm(&self) -> Algorithm {
        self.algo
    }

    /// Set the maximum concurrent client connections (0 = unlimited).
    pub fn max_conns(&self) -> usize {
        self.max_conns.load(Ordering::Relaxed)
    }

    pub fn set_max_conns(&self, max: usize) {
        self.max_conns.store(max, Ordering::Relaxed);
    }

    pub fn active_conns(&self) -> usize {
        self.active_conns.load(Ordering::Relaxed)
    }

    pub fn rejected_conns(&self) -> u64 {
        self.rejected_conns.load(Ordering::Relaxed)
    }

    pub fn stop(&self) {
        self.stop.stop();
    }

    pub fn local_addr(&self) -> std::io::Result<std::net::SocketAddr> {
        self.listener.local_addr()
    }

    pub async fn serve(self: Arc<Self>) {
        loop {
            if self.stop.is_stopped() {
                tracing::info!(pool = %self.name, "tcp load balancer stopping");
                return;
            }
            match self.listener.accept().await {
                Ok((client, _peer)) => {
                    let max = self.max_conns.load(Ordering::Relaxed);
                    if max > 0 && self.active_conns.load(Ordering::Relaxed) >= max {
                        self.rejected_conns.fetch_add(1, Ordering::Relaxed);
                        drop(client); // close immediately
                        continue;
                    }
                    self.active_conns.fetch_add(1, Ordering::Relaxed);

                    let this = self.clone();
                    tokio::spawn(async move {
                        let pool_name = this.name.clone();
                        let tracker = Arc::clone(&this);
                        if let Err(e) = this.handle(client).await {
                            tracing::debug!(pool = %pool_name, error = %e, "connection ended with error");
                        }
                        tracker.active_conns.fetch_sub(1, Ordering::Relaxed);
                    });
                }
                Err(e) => {
                    tracing::warn!(pool = %self.name, error = %e, "accept failed");
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        }
    }

    async fn handle(self: Arc<Self>, mut client: TcpStream) -> std::io::Result<()> {
        let snapshot = self.backends.read().await.clone();
        let max_tries = snapshot.len().max(1);
        for _ in 0..max_tries {
            let Some(be) = select_backend(&snapshot, self.algo, &self.counter) else {
                break;
            };
            match be.connect().await {
                Ok(mut upstream) => {
                    let res = copy_bidirectional(&mut client, &mut upstream).await;
                    if let Ok((up, down)) = res {
                        be.add_bytes(up, down);
                    }
                    be.release();
                    upstream.shutdown().await.ok();
                    return res.map(|_| ());
                }
                Err(e) => {
                    tracing::debug!(pool = %self.name, backend = %be.addr, error = %e, "backend connect failed");
                    // mark unhealthy so health checker re-validates quickly
                    be.set_healthy(false);
                }
            }
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no healthy backends",
        ))
    }
}
