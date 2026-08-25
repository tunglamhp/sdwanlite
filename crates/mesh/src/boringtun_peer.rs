//! Userspace WireGuard data-plane milestone using boringtun.
//!
//! Proves the full Noise-IK handshake works between two sdwanlite nodes
//! without kernel WG or the `wg` tools — the foundation for a portable
//! mesh that later grows into packet forwarding (smoltcp) on any OS.

use crate::MeshError;
use base64::Engine as _;
use boringtun::noise::Tunn;
use boringtun::x25519;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::UdpSocket;

fn decode_key(b64: &str) -> Result<[u8; 32], MeshError> {
    let raw = base64::engine::general_purpose::STANDARD
        .decode(b64.trim())
        .map_err(|_| MeshError::BadKey)?;
    raw.try_into().map_err(|_| MeshError::BadKey)
}

struct Peer {
    #[allow(dead_code)]
    idx: u32,
    tunn: Tunn,
    sock: Arc<UdpSocket>,
    endpoint: std::net::SocketAddr,
}

impl Peer {
    async fn new(
        index: u32,
        private_b64: &str,
        peer_public_b64: &str,
        bind_port: u16,
        endpoint: std::net::SocketAddr,
    ) -> Result<Self, MeshError> {
        let secret = x25519::StaticSecret::from(decode_key(private_b64)?);
        let public = x25519::PublicKey::from(decode_key(peer_public_b64)?);
        eprintln!(
            "[peer-{}] priv_head={:?} peer_pub_head={:?}",
            index,
            &private_b64.as_bytes()[..6],
            &peer_public_b64.as_bytes()[..6]
        );
        let tunn = Tunn::new(secret, public, None, Some(1), index << 8, None);
        let sock = Arc::new(UdpSocket::bind(("0.0.0.0", bind_port)).await?);
        Ok(Self { idx: index, tunn, sock, endpoint })
    }

    /// Send one handshake initiation (or retry).
    pub(crate) async fn kick(&mut self) -> std::io::Result<()> {
        let mut dst = vec![0u8; 148];
        match self.tunn.format_handshake_initiation(&mut dst, false) {
            boringtun::noise::TunnResult::WriteToNetwork(packet) => {
                self.sock.send_to(packet, self.endpoint).await?;
            }
            _ => {}
        }
        Ok(())
    }

    /// Pump inbound datagrams into the tunnel.
    async fn pump(&mut self) -> std::io::Result<()> {
        let mut buf = vec![0u8; 2048];
        let mut out = vec![0u8; 2048];
        while let Ok((n, src)) = self.sock.try_recv_from(&mut buf) {
            let src_ip: Option<std::net::IpAddr> = match src {
                std::net::SocketAddr::V4(v4) => Some(std::net::IpAddr::V4(*v4.ip())),
                std::net::SocketAddr::V6(v6) => Some(std::net::IpAddr::V6(*v6.ip())),
            };
            loop {
                let result = self.tunn.decapsulate(src_ip, &buf[..n], &mut out);
                match result {
                    boringtun::noise::TunnResult::WriteToNetwork(packet) => {
                        self.sock.send_to(packet, self.endpoint).await?;
                    }
                    _ => break,
                }
            }
        }
        Ok(())
    }

    fn time_since_handshake(&self) -> Option<Duration> {
        self.tunn.stats().0
    }
}

/// Drive two in-process peers through a complete WireGuard handshake and
/// return peer A's elapsed-handshake measurement. Pure loopback networking —
/// no kernel interface, no `wg` tools. Works on every platform.
///
/// This is the seed of the portable boringtun mesh: replace the loopback
/// sockets with real endpoints and the same code path drives live tunnels.
pub async fn handshake_smoke(
    priv_a: &str,
    pub_a: &str,
    priv_b: &str,
    pub_b: &str,
) -> Result<Duration, MeshError> {
    // ephemeral ports via binding :0 then reading local_addr
    let sock_a = UdpSocket::bind(("127.0.0.1", 0)).await?;
    let sock_b = UdpSocket::bind(("127.0.0.1", 0)).await?;
    let addr_a = sock_a.local_addr()?;
    let addr_b = sock_b.local_addr()?;
    drop(sock_a);
    drop(sock_b);

    let mut a = Peer::new(1, priv_a, pub_a, addr_a.port(), addr_b).await?;
    let mut b = Peer::new(2, priv_b, pub_b, addr_b.port(), addr_a).await?;

    let deadline = Instant::now() + Duration::from_secs(15);
    a.kick().await?;

    while Instant::now() < deadline {
        a.pump().await?;
        b.pump().await?;

        if let Some(d) = a.time_since_handshake().or_else(|| b.time_since_handshake()) {
            tracing::info!(elapsed = ?d, "boringtun handshake established");
            return Ok(d);
        }

        // re-kick periodically (handshake retries are our job in userspace)
        a.kick().await?;

        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Err(MeshError::Io(std::io::Error::new(
        std::io::ErrorKind::TimedOut,
        "boringtun handshake timed out")))
}


#[cfg(test)]
mod isolate {
    use super::*;
    use boringtun::noise::TunnResult;

    #[test]
    fn initiation_mac_validates_at_responder() {
        let secret_a = x25519::StaticSecret::from([7u8; 32]);
        let pub_a = x25519::PublicKey::from(&secret_a);
        let secret_b = x25519::StaticSecret::from([9u8; 32]);
        let pub_b = x25519::PublicKey::from(&secret_b);

        let mut initiator = Tunn::new(secret_a, pub_b, None, Some(1), 1, None);
        let mut responder = Tunn::new(secret_b, pub_a, None, Some(1), 2, None);

        let mut dst = vec![0u8; 148];
        let pkt = match initiator.format_handshake_initiation(&mut dst, true) {
            TunnResult::WriteToNetwork(p) => p.to_vec(),
            _ => panic!("expected packet"),
        };

        let mut out = vec![0u8; 2048];
        match responder.decapsulate(Some("127.0.0.1".parse().unwrap()), &pkt, &mut out) {
            boringtun::noise::TunnResult::WriteToNetwork(resp) => {
                assert!(resp.len() > 0);
            }
            _other => panic!("unexpected result"),
        }
    }
}
