//! sdwanlite-mesh: WireGuard mesh management.
//!
//! Learning-oriented control plane:
//! - native Curve25519 keypair generation (x25519-dalek),
//! - `wg-quick` configuration rendering,
//! - apply/status via the `wg`/`wg-quick` tools (Linux only).

use base64::Engine;
use rand::rngs::OsRng;
use sdwanlite_core::{Config, Mesh as MeshConfig};
use std::fmt;
use x25519_dalek::{PublicKey, StaticSecret};

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
    let out = run_capture("wg", &["show", "all", "dump"]).await?;
    Ok(parse_wg_dump(&out))
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

