//! sdwanlite-mesh: WireGuard mesh management.
//!
//! Learning-oriented control plane:
//! - native Curve25519 keypair generation (x25519-dalek),
//! - `wg-quick` configuration rendering,
//! - apply/status via the `wg`/`wg-quick` tools (Linux only).

use base64::Engine;
use rand::rngs::OsRng;
use sdwanlite_core::{Mesh as MeshConfig, Peer};
use sdwanlite_core::Config;
use std::fmt;
use x25519_dalek::{PublicKey, StaticSecret};

pub mod boringtun_peer;

#[derive(Debug, thiserror::Error)]
pub enum MeshError {
    #[error("invalid base64 key material")]
    BadKey,
    #[error("WireGuard tools unavailable on this platform (Linux required)")]
    UnsupportedPlatform,
    #[error("wg command failed: {0}")]
    CommandFailed(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone)]
pub struct KeyPair {
    pub private_b64: String,
    pub public_b64: String,
}

/// Generate a fresh WireGuard-compatible Curve25519 keypair.
pub fn generate_keypair() -> KeyPair {
    let secret = StaticSecret::random_from_rng(OsRng);
    let public = PublicKey::from(&secret);
    let b64 = base64::engine::general_purpose::STANDARD;
    KeyPair {
        private_b64: b64.encode(secret.to_bytes()),
        public_b64: b64.encode(public.as_bytes()),
    }
}

fn decode_key(b64: &str) -> Result<[u8; 32], MeshError> {
    let engine = base64::engine::general_purpose::STANDARD;
    let raw = engine.decode(b64.trim()).map_err(|_| MeshError::BadKey)?;
    raw.try_into().map_err(|_| MeshError::BadKey)
}

/// Derive the public key belonging to a base64 private key.
pub fn public_from_private(private_b64: &str) -> Result<String, MeshError> {
    let secret = StaticSecret::from(decode_key(private_b64)?);
    let public = PublicKey::from(&secret);
    Ok(base64::engine::general_purpose::STANDARD.encode(public.as_bytes()))
}

/// Render a `wg-quick` compatible configuration file for interface `ifname`.
pub fn render_wg_config(ifname: &str, mesh: &MeshConfig) -> Result<String, MeshError> {
    if mesh.private_key.is_empty() {
        return Err(MeshError::BadKey);
    }
    // validate key early
    decode_key(&mesh.private_key)?;

    let mut out = format!(
        "[Interface]\nPrivateKey = {}\nListenPort = {}\n",
        mesh.private_key.trim(),
        mesh.listen_port
    );
    for peer in &mesh.peers {
        out.push_str(&format!("\n[Peer]\n# name = {}\nPublicKey = {}\n", peer.name, peer.public_key));
        if let Some(ep) = &peer.endpoint {
            out.push_str(&format!("Endpoint = {ep}\n"));
        }
        if !peer.allowed_ips.is_empty() {
            out.push_str(&format!("AllowedIPs = {}\n", peer.allowed_ips.join(", ")));
        }
        if peer.keepalive_secs > 0 {
            out.push_str(&format!("PersistentKeepalive = {}\n", peer.keepalive_secs));
        }
    }
    let _ = ifname; // wg-quick derives it from the filename
    Ok(out)
}

/// Render a config suitable for `wg setconf` (no ListenPort/Address lines,
/// no [Interface] header - see wg(8)).
pub fn render_wg_setconf(mesh: &MeshConfig) -> Result<String, MeshError> {
    if mesh.private_key.is_empty() {
        return Err(MeshError::BadKey);
    }
    decode_key(&mesh.private_key)?;
    let mut out = format!("private_key = {}\nlisten_port = {}\n", mesh.private_key.trim(), mesh.listen_port);
    for peer in &mesh.peers {
        out.push_str(&format!("\n[Peer]\npublic_key = {}\n", peer.public_key));
        if let Some(ep) = &peer.endpoint {
            out.push_str(&format!("endpoint = {ep}\n"));
        }
        out.push_str(&format!("allowed_ips = {}\n", peer.allowed_ips.join(", ")));
        if peer.keepalive_secs > 0 {
            out.push_str(&format!("persistent_keepalive = {}\n", peer.keepalive_secs));
        }
    }
    Ok(out)
}

/// Pure, cross-platform configuration validation.
pub fn validate(mesh: &MeshConfig) -> Result<(), Vec<String>> {
    let mut problems = Vec::new();
    if mesh.private_key.is_empty() {
        problems.push("private_key is empty".into());
    } else if decode_key(&mesh.private_key).is_err() {
        problems.push("private_key is not valid base64 key material".into());
    }
    let mut seen = std::collections::HashSet::new();
    for p in &mesh.peers {
        if decode_key(&p.public_key).is_err() {
            problems.push(format!("peer '{}': public_key is invalid", p.name));
        }
        if !seen.insert(p.public_key.clone()) {
            problems.push(format!("peer '{}': duplicate public_key", p.name));
        }
        for a in &p.allowed_ips {
            let ok = (|| {
                let (ip, bits) = a.split_once('/')?;
                ip.parse::<std::net::IpAddr>().ok()?;
                bits.parse::<u8>().ok()?;
                Some(())
            })()
            .is_some();
            if !ok {
                problems.push(format!("peer '{}': allowed_ips entry '{a}' is not a valid CIDR", p.name));
            }
        }
        // endpoint sanity when present
        if let Some(ep) = &p.endpoint {
            if ep.rsplit(':').next().map(|port| port.parse::<u16>().is_err()).unwrap_or(true)
                && !ep.contains(':')
            {
                problems.push(format!("peer '{}': endpoint '{ep}' must be host:port", p.name));
            }
        }
    }
    if problems.is_empty() { Ok(()) } else { Err(problems) }
}

/// Apply the mesh configuration with `wg-quick up <path>` (Linux only).
pub async fn apply(cfg: &Config, workdir: &std::path::Path) -> Result<(), MeshError> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (cfg, workdir);
        Err(MeshError::UnsupportedPlatform)
    }

    #[cfg(target_os = "linux")]
    {
        let ifname = "sdwanlite0";
        let conf = render_wg_config(ifname, &cfg.mesh)?;
        let path = workdir.join(format!("{ifname}.conf"));
        tokio::fs::write(&path, conf).await?;
        run("wg-quick", &[ "up", path.to_string_lossy().as_ref() ]).await
    }
}

/// Query live WireGuard status via `wg show all dump` and parse it.
pub async fn status() -> Result<Vec<WgPeerStatus>, MeshError> {
    match run_capture("wg", &["show", "all", "dump"]).await {
        Ok(out) => Ok(parse_wg_dump(&out)),
        Err(MeshError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e),
    }
}

#[derive(Debug, Clone, Default)]
pub struct WgPeerStatus {
    pub interface: String,
    pub public_key: String,
    pub endpoint: Option<String>,
    pub allowed_ips: Vec<String>,
    /// Seconds since latest handshake; None = never.
    pub latest_handshake_secs_ago: Option<u64>,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
}

fn parse_wg_dump(dump: &str) -> Vec<WgPeerStatus> {
    let mut peers = Vec::new();
    let now_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    for line in dump.lines().skip(1) {
        // interface  pubkey  presharedkey  endpoint  allowedips  handshake  rx  tx  keepalive
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < 8 || cols[1].is_empty() {
            continue;
        }
        let handshake = cols[5].parse::<u64>().ok().filter(|t| *t > 0);
        peers.push(WgPeerStatus {
            interface: cols[0].into(),
            public_key: cols[1].into(),
            endpoint: (!cols[3].is_empty()).then(|| cols[3].to_string()),
            allowed_ips: cols[4].split(',').map(str::to_string).collect(),
            latest_handshake_secs_ago: handshake.map(|t| now_unix.saturating_sub(t)),
            rx_bytes: cols[6].parse().unwrap_or(0),
            tx_bytes: cols[7].parse().unwrap_or(0),
        });
    }
    peers
}

/// Apply a full peer/key configuration to an existing interface via `wg setconf`.
pub async fn apply_setconf(mesh: &MeshConfig) -> Result<(), MeshError> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = mesh;
        Err(MeshError::UnsupportedPlatform)
    }
    #[cfg(target_os = "linux")]
    {
        use tokio::io::AsyncWriteExt;
        let conf = render_wg_setconf(mesh)?;
        let mut child = tokio::process::Command::new("wg")
            .args(["setconf", "sdwanlite0"])
            .stdin(std::process::Stdio::piped())
            .spawn()?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(conf.as_bytes()).await?;
        }
        let out = child.wait().await?;
        if out.success() { Ok(()) } else { Err(MeshError::CommandFailed("wg setconf failed".into())) }
    }
}

/// Add or update one peer on the running interface.
pub async fn add_peer(peer: &Peer) -> Result<(), MeshError> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = peer;
        Err(MeshError::UnsupportedPlatform)
    }
    #[cfg(target_os = "linux")]
    {
        let mut args = vec!["set".to_string(), "sdwanlite0".to_string(), "peer".to_string(), peer.public_key.clone()];
        if let Some(ep) = &peer.endpoint {
            args.extend(["endpoint".into(), ep.clone()]);
        }
        if !peer.allowed_ips.is_empty() {
            args.extend(["allowed-ips".into(), peer.allowed_ips.join(",")]);
        }
        if peer.keepalive_secs > 0 {
            args.extend(["persistent-keepalive".into(), peer.keepalive_secs.to_string()]);
        }
        run("wg", &args.iter().map(String::as_str).collect::<Vec<_>>()).await
    }
}

/// Remove one peer from the running interface.
pub async fn remove_peer(public_key_b64: &str) -> Result<(), MeshError> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = public_key_b64;
        Err(MeshError::UnsupportedPlatform)
    }
    #[cfg(target_os = "linux")]
    {
        run("wg", &["set", "sdwanlite0", "peer", public_key_b64, "remove"]).await
    }
}

#[cfg(target_os = "linux")]
async fn run(cmd: &str, args: &[&str]) -> Result<(), MeshError> {
    let out = tokio::process::Command::new(cmd)
        .args(args)
        .output()
        .await?;
    if out.status.success() {
        Ok(())
    } else {
        Err(MeshError::CommandFailed(
            String::from_utf8_lossy(&out.stderr).trim().to_string(),
        ))
    }
}

async fn run_capture(cmd: &str, args: &[&str]) -> Result<String, MeshError> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (cmd, args);
        Err(MeshError::UnsupportedPlatform)
    }
    #[cfg(target_os = "linux")]
    {
        let out = tokio::process::Command::new(cmd).args(args).output().await?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).into_owned())
        } else {
            Err(MeshError::CommandFailed(
                String::from_utf8_lossy(&out.stderr).trim().to_string(),
            ))
        }
    }
}

impl fmt::Display for KeyPair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "public={}", self.public_b64)
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use sdwanlite_core::Mesh;

    #[test]
    fn keypair_is_valid_and_derivable() {
        let kp = generate_keypair();
        let derived = public_from_private(&kp.private_b64).unwrap();
        assert_eq!(derived, kp.public_b64);
    }

    #[test]
    fn renders_wg_quick_config() {
        let kp = generate_keypair();
        let mesh = Mesh {
            enabled: true,
            private_key: kp.private_b64.clone(),
            listen_port: 51820,
            peers: vec![Peer {
                name: "site-b".into(),
                public_key: "invalid".into(), // render does not validate peer keys
                endpoint: Some("203.0.113.2:51820".into()),
                allowed_ips: vec!["10.100.0.2/32".into()],
                keepalive_secs: 25,
            }],
        };
        let conf = render_wg_config("sdwanlite0", &mesh).unwrap();
        assert!(conf.contains(&format!("PrivateKey = {}", kp.private_b64)));
        assert!(conf.contains("ListenPort = 51820"));
        assert!(conf.contains("[Peer]"));
        assert!(conf.contains("AllowedIPs = 10.100.0.2/32"));
        assert!(conf.contains("PersistentKeepalive = 25"));
    }

    #[test]
    fn rejects_empty_private_key() {
        let mesh = Mesh::default();
        assert!(render_wg_config("x", &mesh).is_err());
    }

    #[test]
    fn validate_good_and_bad() {
        let kp = generate_keypair();
        let good = Mesh {
            enabled: true,
            private_key: kp.private_b64.clone(),
            listen_port: 51820,
            peers: vec![Peer {
                name: "b".into(),
                public_key: kp.public_b64.clone(),
                endpoint: Some("203.0.113.2:51820".into()),
                allowed_ips: vec!["10.100.0.2/32".into()],
                keepalive_secs: 0,
            }],
        };
        assert!(validate(&good).is_ok());

        let bad = Mesh {
            enabled: true,
            private_key: "not-base64!!".into(),
            listen_port: 51820,
            peers: vec![
                Peer { name: "dup".into(), public_key: kp.public_b64.clone(), endpoint: None, allowed_ips: vec!["garbage".into()], keepalive_secs: 0 },
                Peer { name: "dup".into(), public_key: kp.public_b64.clone(), endpoint: None, allowed_ips: vec![], keepalive_secs: 0 },
            ],
        };
        let problems = validate(&bad).unwrap_err();
        assert!(problems.iter().any(|p| p.contains("private_key")), "{problems:?}");
        assert!(problems.iter().any(|p| p.contains("duplicate")), "{problems:?}");
        assert!(problems.iter().any(|p| p.contains("allowed_ips")), "{problems:?}");
    }

    #[test]
    fn setconf_render_differs_from_wg_quick() {
        let kp = generate_keypair();
        let mesh = Mesh {
            enabled: true,
            private_key: kp.private_b64.clone(),
            listen_port: 51820,
            peers: vec![],
        };
        let sc = render_wg_setconf(&mesh).unwrap();
        assert!(!sc.contains("[Interface]"));
        assert!(sc.starts_with("private_key"));
    }

    #[test]
    fn parses_dump_with_never_handshaked_peer() {
        // interface pubkey psk endpoint allowedips handshake rx tx ka
        let dump = "interface\tpubkey\tpsk\tendpoint\tallowed\thandshake\trx\ttx\tka\nsdwanlite0\tKEYA\t(none)\t203.0.113.2:51820\t10.100.0.2/32\t1700000000\t1000\t2000\t25\nsdwanlite0\tKEYB\t(none)\t(none)\t10.100.0.3/32\t0\t0\t0\t0\n";
        let peers = parse_wg_dump(dump);
        assert_eq!(peers.len(), 2);
        assert_eq!(peers[0].rx_bytes, 1000);
        assert!(peers[0].latest_handshake_secs_ago.is_some());
        assert!(peers[1].latest_handshake_secs_ago.is_none());
    }
}

#[cfg(test)]
mod boringtun_tests {
    #[tokio::test]
    async fn full_wg_handshake_between_two_inprocess_peers() {
        use crate::boringtun_peer::handshake_smoke;

        // derive a keypair via the existing native generator
        let kp_a = super::generate_keypair();
        let kp_b = super::generate_keypair();

        // cross-wire public keys and run the Noise-IK handshake over loopback
        let elapsed = tokio::time::timeout(
            std::time::Duration::from_secs(20),
            handshake_smoke(&kp_a.private_b64, &kp_b.public_b64, &kp_b.private_b64, &kp_a.public_b64),
        )
        .await
        .expect("handshake within timeout")
        .expect("handshake success");

        assert!(elapsed < std::time::Duration::from_secs(15));
    }
}
