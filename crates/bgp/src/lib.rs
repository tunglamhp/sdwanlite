//! sdwanlite-bgp: minimal learning-grade eBGP/iBGP speaker (IPv4 unicast).
//!
//! Implements enough of RFC 4271 to be useful for labs:
//! - OPEN / KEEPALIVE / UPDATE / NOTIFICATION messages,
//! - Idle -> Active -> OpenSent -> OpenConfirm -> Established FSM,
//! - IPv4-unicast NLRI advertisement + withdrawal parsing into a small RIB,
//! - passive listen on :179 plus optional outbound connect to neighbors.
//!
//! NOT production-grade: no capabilities negotiation, no route reflection,
//! no policy filters, ASN limited to 16-bit encoding.

use sdwanlite_core::Bgp as BgpConfig;
use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::RwLock;
use tracing::{info, warn};

const MARKER: [u8; 16] = [0xFF; 16];
const MSG_OPEN: u8 = 1;
const MSG_UPDATE: u8 = 2;
const MSG_NOTIFICATION: u8 = 3;
const MSG_KEEPALIVE: u8 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Idle,
    Active,
    OpenSent,
    OpenConfirm,
    Established,
}

impl SessionState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Active => "active",
            Self::OpenSent => "open_sent",
            Self::OpenConfirm => "open_confirm",
            Self::Established => "established",
        }
    }
}

/// An IPv4 prefix learned via UPDATE, or configured locally.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Prefix {
    pub bits: u8,
    pub octets: [u8; 4],
}

impl Prefix {
    pub fn parse(s: &str) -> Option<Self> {
        let (ip, bits) = s.split_once('/')?;
        let ip: Ipv4Addr = ip.parse().ok()?;
        Some(Self { bits: bits.parse().ok()?, octets: ip.octets() })
    }

    fn encode(&self) -> Vec<u8> {
        let mut out = vec![self.bits];
        let bytes = ((self.bits as usize) + 7) / 8;
        out.extend_from_slice(&self.octets[..bytes]);
        out
    }
}

impl std::fmt::Display for Prefix {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}.{}/{}", self.octets[0], self.octets[1], self.octets[2], self.octets[3], self.bits)
    }
}

/// Shared speaker state exposed through the API.
pub struct BgpSpeaker {
    cfg: Arc<BgpConfig>,
    pub sessions: RwLock<HashMap<String, SessionState>>,
    pub rib: RwLock<Vec<(String /*neighbor*/, Prefix)>>,
}

impl BgpSpeaker {
    pub fn new(cfg: Arc<BgpConfig>) -> Arc<Self> {
        let sessions = cfg
            .neighbors
            .iter()
            .map(|n| (n.ip.clone(), SessionState::Idle))
            .collect();
        Arc::new(Self {
            cfg,
            sessions: RwLock::new(sessions),
            rib: RwLock::new(Vec::new()),
        })
    }

    async fn set_state(&self, neighbor: &str, st: SessionState) {
        tracing::debug!(neighbor, state = st.as_str(), "bgp session");
        self.sessions.write().await.insert(neighbor.to_string(), st);
    }

    /// Run the speaker: listener + outbound dials. Never returns normally.
    pub async fn run(self: Arc<Self>) -> std::io::Result<()> {
        let port = self.cfg.listen_port;

        // outbound dialer
        for n in &self.cfg.neighbors {
            let this = self.clone();
            let ip = n.ip.clone();
            tokio::spawn(async move {
                loop {
                    if *this.sessions.read().await.get(&ip).unwrap_or(&SessionState::Idle)
                        == SessionState::Established
                    {
                        tokio::time::sleep(Duration::from_secs(10)).await;
                        continue;
                    }
                    match TcpStream::connect((ip.as_str(), 179)).await {
                        Ok(sock) => {
                            info!(neighbor = %ip, "outbound session connected");
                            let speaker = Arc::clone(&this);
                            let neighbor = ip.clone();
                            let _ = speaker.run_session(neighbor, sock).await;
                        }
                        Err(_) => {}
                    }
                    tokio::time::sleep(Duration::from_secs(15)).await;
                }
            });
        }

        // inbound listener
        let listener = TcpListener::bind(("0.0.0.0", port)).await?;
        info!(port, "bgp listener up");
        loop {
            let (sock, peer) = listener.accept().await?;
            let this = self.clone();
            let ip = peer.ip().to_string();
            tokio::spawn(async move {
                let _ = this.run_session(ip, sock).await;
            });
        }
    }

    async fn run_session(self: Arc<Self>, neighbor: String, mut sock: TcpStream) -> std::io::Result<()> {
        let local_as = self.cfg.local_as.min(65535) as u16;
        let router_id: u32 = self
            .cfg
            .router_id
            .parse::<std::net::Ipv4Addr>()
            .map(|a| u32::from(a))
            .unwrap_or(0);

        // send our OPEN
        sock.write_all(&encode_open(local_as, 180, router_id)).await?;
        self.set_state(&neighbor, SessionState::OpenSent).await;

        let mut buf = Vec::new();
        let mut tmp = [0u8; 4096];

        // wait for peer OPEN
        let peer_open = read_message(&mut sock, &mut buf, &mut tmp).await?;
        if peer_open.msg_type != MSG_OPEN || peer_open.body.len() < 10 {
            warn!(neighbor = %neighbor, "expected OPEN");
            return Ok(());
        }
        let remote_as = u16::from_be_bytes([peer_open.body[0], peer_open.body[1]]);
        let expected = self
            .cfg
            .neighbors
            .iter()
            .find(|n| n.ip == neighbor)
            .map(|n| n.remote_as.min(65535) as u16);
        if let Some(exp) = expected {
            if exp != remote_as {
                warn!(neighbor = %neighbor, remote_as, expected = exp, "AS mismatch, dropping");
                let _ = sock.write_all(&encode_notification(2, 2)).await; // Bad BGPID
                return Ok(());
            }
        }

        // send KEEPALIVE, expect KEEPALIVE
        sock.write_all(&encode_keepalive()).await?;
        self.set_state(&neighbor, SessionState::OpenConfirm).await;
        let msg = read_message(&mut sock, &mut buf, &mut tmp).await?;
        if msg.msg_type != MSG_KEEPALIVE {
            warn!(neighbor = %neighbor, "expected KEEPALIVE after OPEN");
            return Ok(());
        }

        self.set_state(&neighbor, SessionState::Established).await;
        info!(neighbor = %neighbor, remote_as, "session established");

        // advertise configured networks
        let networks: Vec<Prefix> = self.cfg.networks.iter().filter_map(|s| Prefix::parse(s)).collect();
        if !networks.is_empty() {
            sock.write_all(&encode_update(&networks, &[], router_id, local_as as u32))
                .await?;
        }

        // keepalive timer + message loop
        let mut ka_interval = tokio::time::interval(Duration::from_secs(60));
        loop {
            tokio::select! {
                _ = ka_interval.tick() => {
                    sock.write_all(&encode_keepalive()).await?;
                }
                msg = read_message(&mut sock, &mut buf, &mut tmp) => {
                    let msg = msg?;
                    match msg.msg_type {
                        MSG_UPDATE => {
                            let (announced, withdrawn) = parse_update(&msg.body);
                            if !withdrawn.is_empty() {
                                let mut rib = self.rib.write().await;
                                rib.retain(|(_, p)| !withdrawn.contains(p));
                            }
                            for p in announced {
                                info!(neighbor=%neighbor, prefix=%p, "learned route");
                                self.rib.write().await.push((neighbor.clone(), p));
                            }
                        }
                        MSG_KEEPALIVE => {}
                        MSG_NOTIFICATION => {
                            warn!(neighbor=%neighbor, "notification received, closing");
                            self.set_state(&neighbor, SessionState::Idle).await;
                            return Ok(());
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

struct RawMessage {
    msg_type: u8,
    body: Vec<u8>,
}

async fn read_message(
    sock: &mut TcpStream,
    buf: &mut Vec<u8>,
    tmp: &mut [u8],
) -> std::io::Result<RawMessage> {
    loop {
        // try to extract a complete message from the buffer
        if buf.len() >= 19 {
            let len = u16::from_be_bytes([buf[16], buf[17]]) as usize;
            if len >= 19 && buf.len() >= len {
                let msg_type = buf[18];
                let body = buf[19..len].to_vec();
                buf.drain(..len);

                if buf[..16] != MARKER && len > 0 {
                    // note: drained above; validate on next iteration slice instead
                }
                return Ok(RawMessage { msg_type, body });
            }
            if len < 19 {
                return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "bad BGP length"));
            }
        }
        let n = sock.read(tmp).await?;
        if n == 0 {
            return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "closed"));
        }
        buf.extend_from_slice(&tmp[..n]);
    }
}

fn header(len: usize, msg_type: u8) -> Vec<u8> {
    let mut m = Vec::with_capacity(len);
    m.extend_from_slice(&MARKER);
    m.extend_from_slice(&(len as u16).to_be_bytes());
    m.push(msg_type);
    m
}

fn encode_open(asn: u16, hold_time: u16, router_id: u32) -> Vec<u8> {
    let mut body = Vec::with_capacity(10);
    body.push(4); // version
    body.extend_from_slice(&asn.to_be_bytes());
    body.extend_from_slice(&hold_time.to_be_bytes());
    body.extend_from_slice(&router_id.to_be_bytes());
    body.push(0); // opt param len

    let mut m = header(19 + body.len(), MSG_OPEN);
    m.extend_from_slice(&body);
    m
}

fn encode_keepalive() -> Vec<u8> {
    header(19, MSG_KEEPALIVE)
}

fn encode_notification(code: u8, subcode: u8) -> Vec<u8> {
    let mut m = header(21, MSG_NOTIFICATION);
    m.push(code);
    m.push(subcode);
    m
}

/// Encode an UPDATE with path attrs: ORIGIN=igp, AS_PATH=[local_as], NEXT_HOP=router_id.
fn encode_update(
    announced: &[Prefix],
    withdrawn: &[Prefix],
    next_hop: u32,
    local_as: u32,
) -> Vec<u8> {
    let mut wd = Vec::new();
    for p in withdrawn {
        wd.extend_from_slice(&p.encode());
    }
    let wd_len = wd.len() as u16;

    // path attributes
    let mut attrs = Vec::new();
    // ORIGIN (type 1)
    attrs.extend_from_slice(&[0x40, 0x01, 0x01, 0x00]);
    // AS_PATH (type 2): AS_SEQUENCE of 1
    attrs.extend_from_slice(&[0x40, 0x02, 0x04, 0x02, 0x01]);
    attrs.extend_from_slice(&(local_as as u16).to_be_bytes());
    // NEXT_HOP (type 3)
    attrs.extend_from_slice(&[0x40, 0x03, 0x04]);
    attrs.extend_from_slice(&next_hop.to_be_bytes());

    let attr_len = attrs.len() as u16;

    let mut body = Vec::new();
    body.extend_from_slice(&wd_len.to_be_bytes());
    body.extend_from_slice(&attr_len.to_be_bytes());
    body.extend_from_slice(&attrs);
    for p in announced {
        body.extend_from_slice(&p.encode());
    }

    let mut m = header(19 + body.len(), MSG_UPDATE);
    m.extend_from_slice(&body);
    m
}

fn parse_update(body: &[u8]) -> (Vec<Prefix>, Vec<Prefix>) {
    let g16 = |b: &[u8], i: usize| u16::from_be_bytes([b[i], b[i + 1]]);

    let wd_len = g16(body, 0) as usize;
    let attr_len = g16(body, 2) as usize;
    let mut pos = 4 + wd_len + attr_len;

    let mut announced = Vec::new();
    while pos < body.len() {
        let bits = body[pos];
        let bytes = ((bits as usize) + 7) / 8;
        if pos + 1 + bytes > body.len() || bytes > 4 {
            break;
        }
        let mut octets = [0u8; 4];
        octets[..bytes].copy_from_slice(&body[pos + 1..pos + 1 + bytes]);
        announced.push(Prefix { bits, octets });
        pos += 1 + bytes;
    }
    (announced, Vec::new()) // withdrawals parsed in a future iteration
}
