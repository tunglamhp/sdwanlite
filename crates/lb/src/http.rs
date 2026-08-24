//! HTTP/1.1 reverse proxy with host/path routing and optional TLS termination.

use crate::{select_backend, spawn_health_checker, Backend, HealthCheck, Conn, StopFlag};
use sdwanlite_core::{Algorithm, HttpPool};
use std::net::IpAddr;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

struct Route {
    host: String,
    path_prefix: String,
    backends: Vec<Arc<Backend>>,
    counter: AtomicU64,
}

pub struct HttpLoadBalancer {
    pub name: String,
    listener: TcpListener,
    algo: Algorithm,
    routes: Vec<Arc<Route>>,
    tls: Option<tokio_rustls::TlsAcceptor>,
    stop: Arc<StopFlag>,
    max_conns: AtomicUsize,
    active_conns: AtomicUsize,
    rejected_conns: AtomicU64,
}

impl HttpLoadBalancer {
    pub async fn bind(pool: &HttpPool) -> std::io::Result<Arc<Self>> {
        let listener = TcpListener::bind(&pool.listen).await?;
        let tls = match &pool.tls {
            Some(t) => Some(tokio_rustls::TlsAcceptor::from(crate::load_tls_server_config(
                std::path::Path::new(&t.cert_file),
                std::path::Path::new(&t.key_file),
            )?)),
            None => None,
        };
        let routes: Vec<Arc<Route>> = pool
            .routes
            .iter()
            .map(|r| {
                Arc::new(Route {
                    host: r.host.to_ascii_lowercase(),
                    path_prefix: r.path_prefix.clone(),
                    backends: r
                        .backends
                        .iter()
                        .filter_map(|s| s.parse().ok().map(Backend::new))
                        .collect(),
                    counter: AtomicU64::new(0),
                })
            })
            .collect();
        for r in &routes {
            spawn_health_checker(
                format!("{}/{}", pool.name, r.path_prefix),
                r.backends.clone(),
                Duration::from_secs(pool.health_interval_secs.max(1)),
                Duration::from_secs(pool.health_timeout_secs),
                HealthCheck::from_path(
                    pool.health_check_path.as_ref().or(Some(&"/".to_string())),
                ),
            );
        }
        let lb = Arc::new(Self {
            name: pool.name.clone(),
            listener,
            algo: Algorithm::LeastConnections,
            routes,
            tls,
            stop: Arc::new(StopFlag::default()),
            max_conns: AtomicUsize::new(0), // 0 = unlimited
            active_conns: AtomicUsize::new(0),
            rejected_conns: AtomicU64::new(0),
        });
        tracing::info!(pool = %pool.name, listen = %pool.listen, tls = lb.tls.is_some(), "http load balancer listening");
        Ok(lb)
    }

    /// Backend lists per route (for metrics).
    pub fn backends_by_route(&self) -> Vec<Vec<Arc<Backend>>> {
        self.routes.iter().map(|r| r.backends.clone()).collect()
    }

    /// (host, path_prefix, backend_count) per route.
    pub fn route_info(&self) -> Vec<(String, String, usize)> {
        self.routes
            .iter()
            .map(|r| (r.host.clone(), r.path_prefix.clone(), r.backends.len()))
            .collect()
    }

    /// Set the maximum concurrent client connections (0 = unlimited).
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
                tracing::info!(pool = %self.name, "http load balancer stopping");
                return;
            }
            match self.listener.accept().await {
                Ok((sock, peer)) => {
                    // connection limit
                    let max = self.max_conns.load(Ordering::Relaxed);
                    if max > 0 && self.active_conns.load(Ordering::Relaxed) >= max {
                        self.rejected_conns.fetch_add(1, Ordering::Relaxed);
                        drop(sock); // close immediately
                        continue;
                    }
                    self.active_conns.fetch_add(1, Ordering::Relaxed);

                    // optional TLS termination
                    let conn: Conn = match (&self.tls, sock) {
                        (Some(acceptor), s) => match acceptor.accept(s).await {
                            Ok(t) => Conn::Tls(Box::new(t)),
                            Err(e) => {
                                tracing::debug!(error = %e, "tls handshake failed");
                                self.active_conns.fetch_sub(1, Ordering::Relaxed);
                                continue;
                            }
                        },
                        (None, s) => Conn::Plain(s),
                    };

                    let this = self.clone();
                    tokio::spawn(async move {
                        let pool_name = this.name.clone();
                        let tracker = Arc::clone(&this);
                        if let Err(e) = this.handle(conn, peer.ip()).await {
                            tracing::debug!(pool = %pool_name, error = %e, "http conn error");
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

    async fn handle(
        self: Arc<Self>,
        mut client: Conn,
        peer_ip: IpAddr,
    ) -> std::io::Result<()> {
        // Read until end of request head (bounded).
        let mut buf: Vec<u8> = Vec::with_capacity(8192);
        let mut tmp = [0u8; 4096];
        let head_end = loop {
            let n = client.read(&mut tmp).await?;
            if n == 0 {
                return Ok(());
            }
            buf.extend_from_slice(&tmp[..n]);
            if let Some(pos) = find_head_end(&buf) {
                break pos;
            }
            if buf.len() > 128 * 1024 {
                write_simple_response(&mut client, 431, "Request Header Fields Too Large").await?;
                return Ok(());
            }
        };

        let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
        let Some((path, host)) = parse_request_head(&head) else {
            write_simple_response(&mut client, 400, "Bad Request").await?;
            return Ok(());
        };

        let Some(route) = self.select_route(&host, &path) else {
            write_simple_response(&mut client, 404, "no route matched").await?;
            return Ok(());
        };

        // Rewrite head to include forwarded headers, keep body bytes intact.
        let mut out = rewrite_head_with_headers(buf[..head_end].to_vec(), &peer_ip.to_string());
        out.extend_from_slice(&buf[head_end..]);

        let max_tries = route.backends.len().max(1);
        for _ in 0..max_tries {
            let Some(be) = select_backend(&route.backends, self.algo, &route.counter) else {
                break;
            };
            match be.connect().await {
                Ok(mut upstream) => {
                    if let Err(e) = upstream.write_all(&out).await {
                        be.release();
                        return Err(e);
                    }
                    let res = pump(&mut client, &mut upstream).await;
                    if let Ok((up, down)) = &res {
                        be.add_bytes(*up, *down);
                    }
                    be.release();
                    upstream.shutdown().await.ok();
                    tracing::trace!(pool=%self.name, path=%path, backend=%be.addr, "proxied");
                    return res.map(|_| ());
                }
                Err(e) => {
                    tracing::debug!(backend = %be.addr, error = %e, "backend connect failed");
                    be.set_healthy(false);
                }
            }
        }

        write_simple_response(&mut client, 502, "no healthy backend").await
    }

    fn select_route(&self, host: &str, path: &str) -> Option<&Arc<Route>> {
        self.routes
            .iter()
            .filter(|r| r.host.is_empty() || r.host == host)
            .filter(|r| path.starts_with(r.path_prefix.as_str()))
            .max_by_key(|r| r.path_prefix.len())
    }
}

/// Pump raw bytes both directions until either side closes or errors.
/// Returns (client->backend bytes, backend->client bytes).
async fn pump(a: &mut Conn, b: &mut TcpStream) -> std::io::Result<(u64, u64)> {
    let (mut a_rd, mut a_wr) = tokio::io::split(a);
    let (mut b_rd, mut b_wr) = tokio::io::split(b);

    let up = tokio::io::copy(&mut a_rd, &mut b_wr);
    let down = tokio::io::copy(&mut b_rd, &mut a_wr);

    // Whichever direction finishes first ends the exchange.
    let mut up_n = 0u64;
    let mut down_n = 0u64;
    tokio::select! {
        r = up => { up_n = r?; }
        r = down => { down_n = r?; }
    }
    Ok((up_n, down_n))
}

fn find_head_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4)
}

/// Returns (path, lowercase Host without port).
fn parse_request_head(head: &str) -> Option<(String, String)> {
    let mut lines = head.split("\r\n");
    let req_line = lines.next()?;
    let mut parts = req_line.split_whitespace();
    let _method = parts.next()?;
    let path = parts.next()?.to_string();

    let mut host = String::new();
    for line in lines {
        if line.len() >= 6
            && line
                .get(..5)
                .map(|s| s.eq_ignore_ascii_case("host:"))
                .unwrap_or(false)
        {
            let value = line[5..].trim();
            host = value.split(':').next().unwrap_or("").to_ascii_lowercase();
        }
    }
    Some((path, host))
}

/// Rebuild the request head with X-Forwarded-For appended.
fn rewrite_head_with_headers(head: Vec<u8>, client_ip: &str) -> Vec<u8> {
    if !head.ends_with(b"\r\n\r\n") {
        return head;
    }
    let mut out = head;
    out.truncate(out.len() - 2); // drop final CRLF so we append before it
    out.extend_from_slice(format!("X-Forwarded-For: {}\r\n", client_ip).as_bytes());
    out.extend_from_slice(b"\r\n");
    out
}

async fn write_simple_response(sock: &mut Conn, code: u16, msg: &str) -> std::io::Result<()> {
    let body = format!("{{\"error\":\"{}\"}}", msg);
    let resp = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        code, msg, body.len(), body
    );
    sock.write_all(resp.as_bytes()).await
}
