//! Renewal loop: keep cert/key files fresh.

use crate::{error::AcmeError, obtain::obtain_certificate};
use sdwanlite_core::Acme;
use std::path::Path;

/// Issue (or re-issue) the certificate and atomically write cert/key files.
/// A `<key_file>.issued` sidecar records issuance time.
pub async fn issue_now(cfg: &Acme) -> Result<(), AcmeError> {
    let (cert, key) = obtain_certificate(cfg).await?;

    // atomic-ish write: temp file + rename
    let cert_tmp = format!("{}.tmp", cfg.cert_file);
    let key_tmp = format!("{}.tmp", cfg.key_file);
    tokio::fs::write(&cert_tmp, &cert).await?;
    tokio::fs::write(&key_tmp, &key).await?;
    tokio::fs::rename(&cert_tmp, &cfg.cert_file).await?;
    tokio::fs::rename(&key_tmp, &cfg.key_file).await?;
    tokio::fs::write(format!("{}.issued", cfg.key_file), now_secs().to_string()).await?;

    tracing::info!(domains = ?cfg.domains, "acme certificate issued");
    Ok(())
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// True when files are missing or older than `max_age_days`.
pub async fn needs_renewal(cfg: &Acme) -> bool {
    if !Path::new(&cfg.cert_file).exists() || !Path::new(&cfg.key_file).exists() {
        return true;
    }
    let issued_path = format!("{}.issued", cfg.key_file);
    let Ok(raw) = tokio::fs::read_to_string(&issued_path).await else {
        return true;
    };
    match raw.trim().parse::<u64>() {
        Ok(issued) => {
            let age_days = now_secs().saturating_sub(issued) / 86_400;
            age_days >= u64::from(cfg.renew_days.max(1))
        }
        Err(_) => true,
    }
}

/// Long-running renewal task. Checks daily; issues when needed. Never returns.
pub async fn renew_loop(cfg: Acme) {
    loop {
        if let Err(e) = async {
            if needs_renewal(&cfg).await {
                issue_now(&cfg).await?;
                tracing::info!("certificate written; reload TLS to pick it up");
            } else {
                tracing::debug!("certificate still fresh; skipping renewal");
            }
            Ok::<(), AcmeError>(())
        }
        .await
        {
            tracing::warn!("acme renewal failed: {e}");
        }
        tokio::time::sleep(std::time::Duration::from_secs(86_400)).await;
    }
}
