//! TLS server-config loading from PEM cert/key files.

use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use std::path::Path;
use std::sync::Arc;

/// Build a rustls server config from PEM files.
pub fn load_tls_server_config(
    cert_file: &Path,
    key_file: &Path,
) -> Result<Arc<rustls::ServerConfig>, std::io::Error> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .ok();

    let certs: Vec<CertificateDer<'_>> = rustls_pemfile::certs(&mut std::io::BufReader::new(
        std::fs::File::open(cert_file)
            .map_err(|e| std::io::Error::new(e.kind(), format!("open cert: {e}")))?,
    ))
    .collect::<Result<_, _>>()?;

    let key: PrivateKeyDer<'_> = rustls_pemfile::private_key(&mut std::io::BufReader::new(
        std::fs::File::open(key_file)
            .map_err(|e| std::io::Error::new(e.kind(), format!("open key: {e}")))?,
    ))?
    .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "no private key found"))?;

    if certs.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "no certificates found",
        ));
    }

    let cfg = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
    Ok(Arc::new(cfg))
}
