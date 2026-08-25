//! Userspace TCP forwarding through the boringtun tunnel.
//!
//! Each [`VpnNode`] owns:
//! - a boringtun [`Tunn`](boringtun::noise::Tunn) (WireGuard data plane),
//! - a smoltcp [`Interface`] with a virtual IPv4 address (userspace TCP/IP),
//! - one UDP socket carrying encrypted WG datagrams.
//!
//! The smoltcp device consumes decrypted IP packets and hands outbound IP
//! packets to the tunnel for encapsulation — giving any OS a portable
//! WG mesh without TUN devices or kernel modules.

use base64::Engine as _;
use boringtun::noise::Tunn;
use boringtun::x25519;
use parking_lot::Mutex;
use smoltcp::iface::{Config as IfConfig, Interface, SocketHandle as SmSocketHandle, SocketSet};
use smoltcp::phy::{Device as PhyDevice, DeviceCapabilities, Medium, RxToken, TxToken};
use smoltcp::socket::tcp;
use smoltcp::time::Instant as SmInstant;
use smoltcp::wire::{HardwareAddress, IpAddress, IpCidr, Ipv4Address};
use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::UdpSocket;

use crate::MeshError;

const MTU: usize = 1384;

// ---------------------------------------------------------------------------
// Shared tunnel state
// ---------------------------------------------------------------------------

struct Shared {
    inbound: Mutex<VecDeque<Vec<u8>>>,
    tunn: Mutex<Tunn>,
    udp: UdpSocket,
    peer: Mutex<SocketAddr>,
}

// ---------------------------------------------------------------------------
// smoltcp tokens
// ---------------------------------------------------------------------------

pub struct VpnRxToken {
    bytes: Vec<u8>,
}

impl RxToken for VpnRxToken {
    fn consume<R, F>(self, f: F) -> R
    where
        F: FnOnce(&[u8]) -> R,
    {
        f(&self.bytes)
    }
}

pub struct VpnTxToken {
    shared: Arc<Shared>,
}

impl TxToken for VpnTxToken {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let mut buf = vec![0u8; len];
        let result = f(&mut buf);
        let mut tunn = self.shared.tunn.lock();
        let mut enc = vec![0u8; len + 128];
        match tunn.encapsulate(&buf[..len], &mut enc) {
            boringtun::noise::TunnResult::WriteToNetwork(pkt) => {
                let peer = *self.shared.peer.lock();
                let _ = self.shared.udp.try_send_to(pkt, peer);
            }
            _ => {}
        }
        result
    }
}

// ---------------------------------------------------------------------------
// Device
// ---------------------------------------------------------------------------

pub struct VpnDevice {
    shared: Arc<Shared>,
}

impl PhyDevice for VpnDevice {
    type RxToken<'a> = VpnRxToken;
    type TxToken<'a> = VpnTxToken;

    fn receive(&mut self, _ts: SmInstant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        let mut q = self.shared.inbound.lock();
        let bytes = q.pop_front()?;
        Some((
            VpnRxToken { bytes },
            VpnTxToken { shared: Arc::clone(&self.shared) },
        ))
    }

    fn transmit(&mut self, _ts: SmInstant) -> Option<Self::TxToken<'_>> {
        Some(VpnTxToken { shared: Arc::clone(&self.shared) })
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.medium = Medium::Ip;
        caps.max_transmission_unit = MTU;
        caps
    }
}

// ---------------------------------------------------------------------------
// Virtual node
// ---------------------------------------------------------------------------

pub struct VpnNode {
    shared: Arc<Shared>,
    device: VpnDevice,
    iface: Interface,
    sockets: SocketSet<'static>,
    pub vip: Ipv4Address,
    started: Instant,
    listener: Option<SmSocketHandle>,
    conn: Option<SmSocketHandle>,
    /// When false, this node never initiates WG handshakes (responder-only).
    initiate: bool,
}

fn decode_key(b64: &str) -> Result<[u8; 32], MeshError> {
    let raw = base64::engine::general_purpose::STANDARD
        .decode(b64.trim())
        .map_err(|_| MeshError::BadKey)?;
    raw.try_into().map_err(|_| MeshError::BadKey)
}

fn sm_now(t: Instant) -> SmInstant {
    SmInstant::from_millis(t.elapsed().as_millis() as i64)
}

impl VpnNode {
    /// Bind the UDP transport and bring up the smoltcp interface.
    pub async fn new(
        vip: [u8; 4],
        private_b64: &str,
        peer_public_b64: &str,
        bind_port: u16,
        peer_udp: SocketAddr,
    ) -> Result<Self, MeshError> {
        let secret = x25519::StaticSecret::from(decode_key(private_b64)?);
        let public = x25519::PublicKey::from(decode_key(peer_public_b64)?);
        let tunn = Tunn::new(secret, public, None, None, 42, None);

        let sock = UdpSocket::bind(("0.0.0.0", bind_port)).await?;
        let peer_udp_addr = peer_udp;
        let shared = Arc::new(Shared {
            inbound: Mutex::new(VecDeque::new()),
            tunn: Mutex::new(tunn),
            udp: sock,
            peer: Mutex::new(peer_udp_addr),
        });
        let mut device = VpnDevice { shared: Arc::clone(&shared) };

        let sockets = SocketSet::new(vec![]);
        let config = IfConfig::new(HardwareAddress::Ip);
        let iface = Interface::new(config, &mut device, sm_now(Instant::now()));
        let vip = Ipv4Address::from(vip);
        let mut node = Self {
            shared,
            device,
            iface,
            sockets,
            vip,
            started: Instant::now(),
            listener: None,
            conn: None,
            initiate: true,
        };
        node.iface.update_ip_addrs(|addrs| {
            addrs.push(IpCidr::new(IpAddress::Ipv4(vip), 24)).unwrap();
        });
        Ok(node)
    }

    /// State of the virtual listener ("established" = a client connected).
    pub fn listener_state(&self) -> &'static str {
        match self.listener {
            Some(h) => {
                let s: &tcp::Socket = self.sockets.get(h);
                format!("{:?}", s.state()).leak() as &'static str
            }
            None => "none",
        }
    }

    /// True once a virtual client completed the TCP handshake.
    pub fn listener_established(&self) -> bool {
        match self.listener {
            Some(h) => {
                let s: &tcp::Socket = self.sockets.get(h);
                s.state() == tcp::State::Established
            }
            None => false,
        }
    }

    /// Alias: set up forwarding listener on virtual port.
    pub fn serve_forward(&mut self, vport: u16) {
        self.tcp_listen_forward(vport);
    }

    /// Connection state name for a handle.
    pub fn tcp_state(&self, h: SmSocketHandle) -> &'static str {
        let s: &tcp::Socket = self.sockets.get(h);
        match s.state() {
            tcp::State::Established => "established",
            tcp::State::Closed => "closed",
            tcp::State::Listen => "listening",
            _ => "transition",
        }
    }

    /// Non-blocking read on the accepted listener connection.
    pub fn tcp_try_read_listener(&mut self, buf: &mut [u8]) -> Option<usize> {
        let h = self.listener?;
        let s: &mut tcp::Socket = self.sockets.get_mut(h);
        if !s.can_recv() {
            return None;
        }
        s.recv_slice(buf).ok()
    }

    /// Non-blocking write to the accepted listener connection.
    pub fn tcp_try_write_listener(&mut self, data: Vec<u8>) -> Option<usize> {
        let h = self.listener?;
        let s: &mut tcp::Socket = self.sockets.get_mut(h);
        if !s.can_send() {
            return None;
        }
        s.send_slice(&data).ok()
    }

    /// Update the peer endpoint after creation.
    /// Explicitly emit a WireGuard handshake initiation to the peer.
    pub fn send_handshake_init(&mut self) -> bool {
        let mut tunn = self.shared.tunn.lock();
        let mut enc = vec![0u8; MTU + 128];
        match tunn.format_handshake_initiation(&mut enc, true) {
            boringtun::noise::TunnResult::WriteToNetwork(pkt) => {
                let peer = *self.shared.peer.lock();
                let _ = self.shared.udp.try_send_to(pkt, peer);
                true
            }
            _ => false,
        }
    }

    /// Make this node responder-only (never initiates handshakes).
    pub fn set_passive(&mut self) {
        self.initiate = false;
    }

    /// Update the peer endpoint after creation.
    pub fn set_peer(&mut self, addr: SocketAddr) {
        *self.shared.peer.lock() = addr;
    }

    pub fn local_udp_port(&self) -> u16 {
        self.shared.udp.local_addr().map(|a| a.port()).unwrap_or(0)
    }

    fn now(&self) -> SmInstant {
        SmInstant::from_millis(self.started.elapsed().as_millis() as i64)
    }

    /// One poll cycle: network -> decrypt -> smoltcp -> encrypt -> network.
    pub fn poll(&mut self) {
        // inbound datagrams -> decapsulate
        {
            let mut buf = vec![0u8; 2048];
            while let Ok((n, src)) = self.shared.udp.try_recv_from(&mut buf) {
                let src_ip = match src {
                    SocketAddr::V4(v4) => Some(std::net::IpAddr::V4(*v4.ip())),
                    SocketAddr::V6(v6) => Some(std::net::IpAddr::V6(*v6.ip())),
                };
                let mut tunn = self.shared.tunn.lock();
                let mut out = vec![0u8; MTU + 128];
                loop {
                    match tunn.decapsulate(src_ip, &buf[..n], &mut out) {
                        boringtun::noise::TunnResult::WriteToNetwork(pkt) => {
                            let _ = self.shared.udp.try_send_to(pkt, src);
                        }

                        boringtun::noise::TunnResult::WriteToTunnelV4(pkt, _) => {
                            self.shared.inbound.lock().push_back(pkt.to_vec());
                            break;
                        }
                        boringtun::noise::TunnResult::WriteToTunnelV6(pkt, _) => {
                            self.shared.inbound.lock().push_back(pkt.to_vec());
                            break;
                        }
                        _ => break,
                    }
                }
            }
        }

        // tunnel timers (handshake initiation/retries) - responders skip
        if self.initiate {
            let mut tunn = self.shared.tunn.lock();
            let mut enc = vec![0u8; MTU + 128];
            match tunn.update_timers(&mut enc) {
                boringtun::noise::TunnResult::WriteToNetwork(pkt) => {
                    let peer = *self.shared.peer.lock();
                    let _ = self.shared.udp.try_send_to(pkt, peer);
                }
                _ => {}
            }
        }

        // smoltcp
        self.iface.poll(self.now(), &mut self.device, &mut self.sockets);
    }

    /// Listen for one virtual TCP connection on `vip:vport`.
    pub fn tcp_listen_forward(&mut self, vport: u16) {
        let tcp = tcp::Socket::new(
            tcp::SocketBuffer::new(vec![0; 4096]),
            tcp::SocketBuffer::new(vec![0; 4096]),
        );
        let h: SmSocketHandle = self.sockets.add(tcp);
        let sock: &mut tcp::Socket = self.sockets.get_mut::<tcp::Socket>(h);
        sock.listen(vport).unwrap();
        self.listener = Some(h);
    }

    /// Open an outgoing virtual connection.
    pub fn tcp_open(
        &mut self,
        dst: Ipv4Address,
        vport: u16,
    ) -> Result<SmSocketHandle, MeshError> {
        let tcp = tcp::Socket::new(
            tcp::SocketBuffer::new(vec![0; 4096]),
            tcp::SocketBuffer::new(vec![0; 4096]),
        );
        let h = self.sockets.add(tcp);
        let local_port = 40000u16 + (std::ptr::from_ref(&h) as usize % 1000) as u16;
        let local: Ipv4Address = Ipv4Address::new(10, 7, 0, 1);
        self.sockets
            .get_mut::<tcp::Socket>(h)
            .connect(
                &mut self.iface.context(),
                (dst, vport),
                (local, local_port),
            )
            .map_err(|e| {
                MeshError::Io(std::io::Error::new(std::io::ErrorKind::Other, format!("{e}")))
            })?;
        Ok(h)
    }

    /// Non-blocking read on a handle. Returns Some(0) on close.
    pub fn tcp_try_read(&mut self, h: SmSocketHandle, buf: &mut [u8]) -> Option<usize> {
        let s = self.sockets.get_mut::<tcp::Socket>(h);
        if !s.can_recv() {
            if s.state() == tcp::State::Closed {
                return Some(0);
            }
            return None;
        }
        s.recv_slice(buf).ok()
    }

    /// Non-blocking write on a handle.
    pub fn tcp_try_write(&mut self, h: SmSocketHandle, data: &[u8]) -> Option<usize> {
        let s = self.sockets.get_mut::<tcp::Socket>(h);
        if !s.can_send() {
            return Some(0);
        }
        s.send_slice(data).ok()
    }

    /// Close + remove a socket handle and (optionally) re-arm the listener.
    pub fn tcp_close(&mut self, h: SmSocketHandle) {
        self.sockets.get_mut::<tcp::Socket>(h).close();
        self.sockets.remove(h);
        self.conn = None;
    }

    /// Re-arm the virtual listener after a connection ends.
    pub fn rearm_listener(&mut self, vport: u16) {
        self.tcp_listen_forward(vport);
    }
}
