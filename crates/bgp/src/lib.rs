//! sdwanlite-bgp: minimal learning-grade eBGP/iBGP speaker (IPv4 unicast).
//!
//! Implements a practical subset of RFC 4271 (+ RFC 5492 capabilities,
//! RFC 2918 route refresh) suitable for labs:
//! - OPEN / KEEPALIVE / UPDATE / NOTIFICATION / ROUTE-REFRESH messages,
//! - Idle -> OpenSent -> OpenConfirm -> Established FSM,
//! - capability negotiation (4-octet ASN, route refresh),
//! - negotiated hold time with keepalive = hold/3 and expiry detection,
//! - IPv4-unicast NLRI advertise/withdraw into a small RIB with an
//!   optional import allowlist.
//!
//! NOT production-grade: no path selection beyond first-come, no reflection.

use sdwanlite_core::Bgp as BgpConfig;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::RwLock;
use tracing::{info, warn};

const MARKER: [u8; 16] = [0xFF; 16];
const MSG_OPEN: u8 = 1;
const MSG_UPDATE: u8 = 2;
const MSG_NOTIFICATION: u8 = 3;
const MSG_KEEPALIVE: u8 = 4;
const MSG_ROUTE_REFRESH: u8 = 5;

const CAP_AS4: u8 = 65;
const CAP_ROUTE_REFRESH: u8 = 2;
const AS_TRANS: u16 = 23456;

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

/// Per-neighbor observable session data.
#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub state: SessionState,
    pub remote_as: Option<u32>,
    pub supports_as4: bool,
    pub supports_route_refresh: bool,
    pub negotiated_hold_secs: u16,
    pub prefixes_received: u64,
    pub updates_received: u64,
    /// Times the session left Established unexpectedly.
    pub flaps: u64,
}

impl Default for SessionInfo {
    fn default() -> Self {
        Self {
            state: SessionState::Idle,
            remote_as: None,
            supports_as4: false,
            supports_route_refresh: false,
            negotiated_hold_secs: 180,
            prefixes_received: 0,
            updates_received: 0,
            flaps: 0,
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
        let ip: std::net::Ipv4Addr = ip.parse().ok()?;
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
        write!(
            f,
            "{}.{}.{}.{}/{}",
            self.octets[0], self.octets[1], self.octets[2], self.octets[3], self.bits
        )
    }
}

/// Shared speaker state exposed through the API.
pub struct BgpSpeaker {
    cfg: Arc<BgpConfig>,
    pub sessions: RwLock<HashMap<String, SessionInfo>>,
    /// Best route per prefix (lowest AS-path length wins; ties -> first seen).
    pub rib: RwLock<HashMap<Prefix, RibEntry>>,
}

/// One candidate route.
#[derive(Debug, Clone)]
pub struct Route {
    pub neighbor: String,
    pub as_path_len: u32,
    /// LOCAL_PREF from the neighbor policy (higher wins first).
    pub local_pref: u32,
}

/// All routes for a prefix; `best()` selects the shortest AS-path.
#[derive(Debug, Clone, Default)]
pub struct RibEntry {
    pub routes: Vec<Route>,
}

impl RibEntry {
    /// Best = highest LOCAL_PREF, then shortest AS_PATH.
    pub fn best(&self) -> Option<&Route> {
        self.routes.iter().min_by_key(|r| (std::cmp::Reverse(r.local_pref), r.as_path_len))
    }
    pub fn best_len(&self) -> u32 {
        self.best().map(|r| r.as_path_len).unwrap_or(u32::MAX)
    }
}

impl BgpSpeaker {
    pub fn new(cfg: Arc<BgpConfig>) -> Arc<Self> {
        let sessions = cfg
            .neighbors
            .iter()
            .map(|n| (n.ip.clone(), SessionInfo::default()))
            .collect();
        Arc::new(Self { cfg, sessions: RwLock::new(sessions), rib: RwLock::new(HashMap::new()) })
    }

    async fn set_state(&self, neighbor: &str, st: SessionState) {
        tracing::debug!(neighbor, state = st.as_str(), "bgp session");
        let mut s = self.sessions.write().await;
        let e = s.entry(neighbor.to_string()).or_default();
        if e.state == SessionState::Established && st != SessionState::Established {
            e.flaps += 1;
        }
        e.state = st;
    }

    async fn update_session(&self, neighbor: &str, f: impl FnOnce(&mut SessionInfo)) {
        let mut s = self.sessions.write().await;
        f(s.entry(neighbor.to_string()).or_default());
    }

    /// Run the speaker: listener + outbound dials. Never returns normally.
    pub async fn run(self: Arc<Self>) -> std::io::Result<()> {
        let port = self.cfg.listen_port;

        for n in &self.cfg.neighbors {
            let this = self.clone();
            let ip = n.ip.clone();
            tokio::spawn(async move {
                loop {
                    let st = this
                        .sessions
                        .read()
                        .await
                        .get(&ip)
                        .map(|i| i.state)
                        .unwrap_or(SessionState::Idle);
                    if st == SessionState::Established {
                        tokio::time::sleep(Duration::from_secs(10)).await;
                        continue;
                    }
                    if let Ok(sock) = TcpStream::connect((ip.as_str(), 179)).await {
                        info!(neighbor = %ip, "outbound session connected");
                        let speaker = Arc::clone(&this);
                        let neighbor = ip.clone();
                        let _ = speaker.run_session(neighbor, sock).await;
                    }
                    tokio::time::sleep(Duration::from_secs(15)).await;
                }
            });
        }

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
        let local_as = self.cfg.local_as;
        let as16 = if local_as > 0xFFFF { AS_TRANS } else { local_as as u16 };
        let router_id: u32 = self
            .cfg
            .router_id
            .parse::<std::net::Ipv4Addr>()
            .map(u32::from)
            .unwrap_or(0);
        let want_hold = self.cfg.hold_time_secs.max(3);

        // our OPEN carries capabilities: AS4 (always) + route refresh
        sock.write_all(&encode_open(as16, want_hold, router_id, local_as))
            .await?;
        self.set_state(&neighbor, SessionState::OpenSent).await;

        let mut buf = Vec::new();
        let mut tmp = [0u8; 4096];

        // wait for peer OPEN
        let peer_open = read_message(&mut sock, &mut buf, &mut tmp).await?;
        if peer_open.msg_type != MSG_OPEN || peer_open.body.len() < 10 {
            warn!(neighbor = %neighbor, "expected OPEN");
            return Ok(());
        }
        let open_as16 = u16::from_be_bytes([peer_open.body[0], peer_open.body[1]]);
        let remote_hold = u16::from_be_bytes([peer_open.body[2], peer_open.body[3]]);
        let caps = parse_capabilities(&peer_open.body[10..]);

        let mut sess = SessionInfo {
            supports_as4: caps.as4.is_some(),
            supports_route_refresh: caps.route_refresh,
            ..Default::default()
        };
        // effective remote AS: prefer AS4 capability, else the 16-bit field
        let effective_remote_as = caps.as4.unwrap_or(open_as16 as u32);
        sess.remote_as = Some(effective_remote_as);
        sess.negotiated_hold_secs = want_hold.min(remote_hold.max(3));
        self.update_session(&neighbor, |e| *e = sess).await;

        // verify configured AS
        if let Some(n) = self.cfg.neighbors.iter().find(|n| n.ip == neighbor) {
            if n.remote_as != effective_remote_as {
                warn!(neighbor = %neighbor, expected = n.remote_as, "AS mismatch, dropping");
                let _ = sock.write_all(&encode_notification(2, 2)).await;
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
        let negotiated_hold = {
            let s = self.sessions.read().await;
            s.get(&neighbor).map(|i| i.negotiated_hold_secs).unwrap_or(want_hold)
        };
        info!(neighbor = %neighbor, remote_as = ?effective_remote_as, hold = negotiated_hold, "session established");

        // advertise configured networks, filtered by this neighbor's export policy
        let all_networks: Vec<Prefix> =
            self.cfg.networks.iter().filter_map(|s| Prefix::parse(s)).collect();
        let export: Option<&Vec<String>> = self
            .cfg
            .neighbors
            .iter()
            .find(|n| n.ip == neighbor)
            .and_then(|n| n.export_allowlist.as_ref());
        let networks: Vec<Prefix> = match export {
            Some(list) if !list.is_empty() => all_networks
                .into_iter()
                .filter(|p| list.iter().any(|a| Prefix::parse(a) == Some(p.clone())))
                .collect(),
            Some(_) => Vec::new(), // explicit empty export list = advertise nothing
            None => all_networks,
        };
        if !networks.is_empty() {
            sock.write_all(&encode_update(&networks, &[], router_id, local_as))
                .await?;
        }

        // keepalive at hold/3 + expiry enforcement
        let ka_every = Duration::from_secs((negotiated_hold / 3).max(1) as u64);
        let mut ka_interval = tokio::time::interval(ka_every);
        let mut last_rx = Instant::now();

        loop {
            tokio::select! {
                _ = ka_interval.tick() => {
                    if last_rx.elapsed() > Duration::from_secs(negotiated_hold as u64) {
                        warn!(neighbor=%neighbor, "hold timer expired");
                        let _ = sock.write_all(&encode_notification(4, 0)).await;
                        self.set_state(&neighbor, SessionState::Idle).await;
                        return Ok(());
                    }
                    sock.write_all(&encode_keepalive()).await?;
                }
                msg = read_message(&mut sock, &mut buf, &mut tmp) => {
                    last_rx = Instant::now();
                    let msg = msg?;
                    match msg.msg_type {
                        MSG_UPDATE => {
                            self.update_session(&neighbor, |e| e.updates_received += 1).await;
                            let (announced, withdrawn) = parse_update(&msg.body);
                            {
                                let mut rib = self.rib.write().await;
                                for w in &withdrawn {
                                    // withdrawal removes the route from that neighbor only
                                    if let Some(entry) = rib.get_mut(w) {
                                        entry.routes.retain(|r| r.neighbor != neighbor);
                                        if entry.routes.is_empty() {
                                            rib.remove(w);
                                        }
                                    }
                                }
                                for (p, plen) in announced {
                                    // import policy: per-neighbor override, else global exact-match list
                                    let allowlist: &[String] = self
                                        .cfg
                                        .neighbors
                                        .iter()
                                        .find(|n| n.ip == neighbor)
                                        .map(|n| n.effective_import(&self.cfg.import_allowlist))
                                        .unwrap_or(&self.cfg.import_allowlist);
                                    if !allowlist.is_empty()
                                        && !allowlist.iter().any(|a| Prefix::parse(a) == Some(p.clone()))
                                    {
                                        tracing::debug!(neighbor=%neighbor, prefix=%p, "filtered by import allowlist");
                                        continue;
                                    }
                                    info!(neighbor=%neighbor, prefix=%p, as_path=plen, "learned route");
                                    self.update_session(&neighbor, |e| e.prefixes_received += 1).await;
                                    let lp = self
                                        .cfg
                                        .neighbors
                                        .iter()
                                        .find(|n| n.ip == neighbor)
                                        .map(|n| n.local_pref)
                                        .unwrap_or(100);
                                    let entry = rib.entry(p.clone()).or_default();
                                    match entry.routes.iter_mut().find(|r| r.neighbor == neighbor) {
                                        Some(r) => {
                                            r.as_path_len = plen;
                                            r.local_pref = lp;
                                        }
                                        None => {
                                            if !self.cfg.multipath {
                                                // single-path mode: drop worse routes on insert
                                                if entry.best().map(|b| (std::cmp::Reverse(b.local_pref), b.as_path_len)) <= Some((std::cmp::Reverse(lp), plen)) && !entry.routes.is_empty() {
                                                    tracing::debug!(prefix=%p, "kept better existing route");
                                                } else {
                                                    entry.routes.clear();
                                                }
                                            }
                                            entry.routes.push(Route { neighbor: neighbor.clone(), as_path_len: plen, local_pref: lp });
                                        }
                                    }
                                }
                            }
                        }
                        MSG_KEEPALIVE => {}
                        MSG_ROUTE_REFRESH => {
                            info!(neighbor = %neighbor, "route refresh received");
                            if !networks.is_empty() {
                                sock.write_all(&encode_update(&networks, &[], router_id, local_as)).await?;
                            }
                        }
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
        if buf.len() >= 19 {
            let len = u16::from_be_bytes([buf[16], buf[17]]) as usize;
            if len >= 19 && buf.len() >= len {
                if buf[..16] != MARKER {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "bad BGP marker",
                    ));
                }
                let msg_type = buf[18];
                let body = buf[19..len].to_vec();
                buf.drain(..len);
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

fn encode_open(as16: u16, hold_time: u16, router_id: u32, as4: u32) -> Vec<u8> {
    // Optional parameters: one parameter, type 2 (capabilities), containing
    //   - AS4 capability (code 65, len 4)
    //   - Route Refresh capability (code 2, len 0)
    let mut caps = Vec::new();
    caps.extend_from_slice(&[CAP_AS4, 4]);
    caps.extend_from_slice(&as4.to_be_bytes());
    caps.extend_from_slice(&[CAP_ROUTE_REFRESH, 0]);

    let mut body = Vec::with_capacity(10 + 2 + caps.len());
    body.push(4); // version
    body.extend_from_slice(&as16.to_be_bytes());
    body.extend_from_slice(&hold_time.to_be_bytes());
    body.extend_from_slice(&router_id.to_be_bytes());
    body.push(caps.len() as u8 + 2); // opt param len: header(2) + caps
    body.push(2); // capability advertisement
    body.push(caps.len() as u8);
    body.extend_from_slice(&caps);

    let mut m = header(19 + body.len(), MSG_OPEN);
    m.extend_from_slice(&body);
    m
}

#[derive(Debug, Default, Clone, Copy)]
struct PeerCapabilities {
    as4: Option<u32>,
    route_refresh: bool,
}

fn parse_capabilities(opt: &[u8]) -> PeerCapabilities {
    let mut out = PeerCapabilities::default();
    let mut pos = 0usize;
    while pos + 2 <= opt.len() {
        let plen = opt[pos + 1] as usize;
        let param_type = opt[pos];
        let data = opt.get(pos + 2..pos + 2 + plen).unwrap_or(&[]);
        if param_type == 2 {
            // capabilities list
            let mut cpos = 0usize;
            while cpos + 2 <= data.len() {
                let code = data[cpos];
                let clen = data[cpos + 1] as usize;
                let val = data.get(cpos + 2..cpos + 2 + clen).unwrap_or(&[]);
                match (code, val.len()) {
                    (CAP_AS4, 4) => {
                        out.as4 = Some(u32::from_be_bytes([val[0], val[1], val[2], val[3]]));
                    }
                    (CAP_ROUTE_REFRESH, _) => out.route_refresh = true,
                    _ => {}
                }
                cpos += 2 + clen;
            }
        }
        pos += 2 + plen;
    }
    out
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

/// Encode an UPDATE with path attrs: ORIGIN=igp, AS_PATH=[local_as], NEXT_HOP.
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

    let mut attrs = Vec::new();
    attrs.extend_from_slice(&[0x40, 0x01, 0x01, 0x00]); // ORIGIN igp
    attrs.extend_from_slice(&[0x40, 0x02, 0x04, 0x02, 0x01]); // AS_PATH seq(1)
    attrs.extend_from_slice(&(local_as.min(0xFFFF) as u16).to_be_bytes());
    attrs.extend_from_slice(&[0x40, 0x03, 0x04]); // NEXT_HOP
    attrs.extend_from_slice(&next_hop.to_be_bytes());

    let attr_len = attrs.len() as u16;

    let mut body = Vec::new();
    body.extend_from_slice(&wd_len.to_be_bytes());
    body.extend_from_slice(&attr_len.to_be_bytes());
    body.extend_from_slice(&wd);
    body.extend_from_slice(&attrs);
    for p in announced {
        body.extend_from_slice(&p.encode());
    }

    let mut m = header(19 + body.len(), MSG_UPDATE);
    m.extend_from_slice(&body);
    m
}

fn parse_prefixes(buf: &[u8]) -> Vec<Prefix> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    while pos < buf.len() {
        let bits = buf[pos];
        let bytes = ((bits as usize) + 7) / 8;
        if bytes > 4 || pos + 1 + bytes > buf.len() {
            break; // malformed tail; stop conservatively
        }
        let mut octets = [0u8; 4];
        octets[..bytes].copy_from_slice(&buf[pos + 1..pos + 1 + bytes]);
        out.push(Prefix { bits, octets });
        pos += 1 + bytes;
    }
    out
}

fn parse_update(body: &[u8]) -> (Vec<(Prefix, u32)>, Vec<Prefix>) {
    if body.len() < 4 {
        return (Vec::new(), Vec::new());
    }
    let g16 = |b: &[u8], i: usize| u16::from_be_bytes([b[i], b[i + 1]]);

    let wd_len = g16(body, 0) as usize;
    let attr_len = g16(body, 2) as usize;
    if 4 + wd_len + attr_len > body.len() {
        return (Vec::new(), Vec::new());
    }

    let withdrawn = parse_prefixes(&body[4..4 + wd_len]);

    // walk path attributes to find AS_PATH (type 2) length
    let attrs = &body[4 + wd_len..4 + wd_len + attr_len];
    let mut as_path_len = 0u32;
    let mut pos = 0usize;
    while pos + 2 <= attrs.len() {
        let flags = attrs[pos];
        let atype = attrs[pos + 1];
        let len = if flags & 0x10 != 0 {
            // extended length
            match attrs.get(pos + 2..pos + 4) {
                Some(b) => u16::from_be_bytes([b[0], b[1]]) as usize,
                None => break,
            }
        } else {
            attrs[pos + 2] as usize
        };
        let hdr = if flags & 0x10 != 0 { 4 } else { 3 };
        if atype == 2 && data_ok(attrs, pos + hdr, len) {
            // count ASes across segments: seg type(1) count(1) then 2/4 bytes each
            let data = &attrs[pos + hdr..pos + hdr + len];
            let mut sp = 0usize;
            while sp + 2 <= data.len() {
                let seg_type = data[sp];
                let count = data[sp + 1] as usize;
                let width = if seg_type == 1 || seg_type == 3 { 2 } else { 4 }; // AS_SEQUENCE/AS_SET vs AS4
                as_path_len += count as u32;
                sp += 2 + count * width;
            }
        }
        pos += hdr + len;
    }

    fn data_ok(b: &[u8], off: usize, len: usize) -> bool {
        off + len <= b.len()
    }

    let announced = parse_prefixes(&body[4 + wd_len + attr_len..])
        .into_iter()
        .map(|p| (p, as_path_len))
        .collect();
    (announced, withdrawn)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_roundtrip() {
        let p = Prefix::parse("10.1.0.0/16").unwrap();
        assert_eq!(p.bits, 16);
        assert_eq!(p.octets, [10, 1, 0, 0]);
        assert_eq!(p.to_string(), "10.1.0.0/16");
    }

    #[test]
    fn update_encode_parse_roundtrip() {
        let announced = vec![
            Prefix::parse("192.168.1.0/24").unwrap(),
            Prefix::parse("10.0.0.0/8").unwrap(),
        ];
        let withdrawn = vec![Prefix::parse("172.16.0.0/12").unwrap()];
        let msg = encode_update(&announced, &withdrawn, 0x0a000001, 65000);

        let total = u16::from_be_bytes([msg[16], msg[17]]) as usize;
        assert_eq!(msg[18], MSG_UPDATE);
        let (got_ann, got_wd) = parse_update(&msg[19..total]);
        assert_eq!(got_wd, withdrawn);
        let got_ann_pfx: Vec<Prefix> = got_ann.iter().map(|(p, _)| p.clone()).collect();
        assert_eq!(got_ann_pfx, announced);
        // single-AS path from encode_update -> len 1
        assert!(got_ann.iter().all(|(_, l)| *l == 1));
    }

    #[test]
    fn open_carries_capabilities_and_parses_back() {
        let m = encode_open(AS_TRANS, 90, 0x0a000001, 4200000000);
        let len = u16::from_be_bytes([m[16], m[17]]) as usize;
        assert_eq!(len, m.len());
        assert_eq!(m[18], MSG_OPEN);
        assert_eq!(&m[..16], &MARKER);
        assert_eq!(u16::from_be_bytes([m[20], m[21]]), AS_TRANS); // ASN field

        let caps = parse_capabilities(&m[29..len]); // 19 hdr + 10 fixed body
        assert_eq!(caps.as4, Some(4200000000));
        assert!(caps.route_refresh);
    }

    #[test]
    fn hold_time_negotiation_is_min_of_both_sides() {
        // pure math mirror of session logic
        let want = 180u16;
        let remote = 60u16;
        assert_eq!(want.min(remote.max(3)), 60);
        let tiny_remote = 1u16;
        assert_eq!(want.min(tiny_remote.max(3)), 3);
    }
}

#[cfg(test)]
mod policy_tests {
    use super::*;

    #[test]
    fn local_pref_beats_shorter_path() {
        let entry = RibEntry {
            routes: vec![
                Route { neighbor: "a".into(), as_path_len: 1, local_pref: 50 },
                Route { neighbor: "b".into(), as_path_len: 5, local_pref: 200 },
            ],
        };
        assert_eq!(entry.best().unwrap().neighbor, "b");
    }

    #[test]
    fn equal_pref_shortest_path_wins() {
        let entry = RibEntry {
            routes: vec![
                Route { neighbor: "long".into(), as_path_len: 7, local_pref: 100 },
                Route { neighbor: "short".into(), as_path_len: 2, local_pref: 100 },
            ],
        };
        assert_eq!(entry.best().unwrap().neighbor, "short");
    }
}
