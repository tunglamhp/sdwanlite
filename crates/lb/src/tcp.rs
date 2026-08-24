//! Layer-4 TCP load balancer with transparent bidirectional forwarding.

use crate::{select_backend, spawn_health_checker, Backend};
use sdwanlite_core::{Algorithm, TcpPool};
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{copy_bidirectional, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

pub struct TcpLoadBalancer {
    pub name: String,
    listener: TcpListener,
    algo: Algorithm,
    backends: Vec<Arc<Backend>>,
    counter: AtomicU64,
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
            backends,
            counter: AtomicU64::new(0),
        });
        spawn_health_checker(
            pool.name.clone(),
            lb.backends.clone(),
            Duration::from_secs(pool.health_interval_secs),
            Duration::from_secs(pool.health_timeout_secs),
        );
        tracing::info!(pool = %pool.name, listen = %pool.listen, "tcp load balancer listening");
        Ok(lb)
    }

    pub fn backends(&self) -> &[Arc<Backend>] {
        &self.backends
    }

    pub fn algorithm(&self) -> Algorithm {
        self.algo
    }

    pub async fn serve(self: Arc<Self>) {
        loop {
            match self.listener.accept().await {
                Ok((client, _peer)) => {
                    let this = self.clone();
                    tokio::spawn(async move {
                        let pool = this.name.clone();
                        if let Err(e) = this.handle(client).await {
                            tracing::debug!(pool = %pool, error = %e, "connection ended with error");
                        }
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
        let max_tries = self.backends.len().max(1);
        for _ in 0..max_tries {
            let Some(be) = select_backend(&self.backends, self.algo, &self.counter) else {
                break;
            };
            match be.connect().await {
                Ok(mut upstream) => {
                    let res = copy_bidirectional(&mut client, &mut upstream).await;
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
