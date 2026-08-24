//! Certificate issuance over ACME with the HTTP-01 challenge.

use crate::error::AcmeError;
use instant_acme::{Account, Identifier, NewAccount, NewOrder, ChallengeType, OrderStatus};
use sdwanlite_core::Acme;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// A running HTTP-01 challenge responder. Drop/abort it when done.
pub struct ChallengeServer {
    handle: tokio::task::JoinHandle<()>,
}

impl ChallengeServer {
    /// Serve `key_auth` values under `/.well-known/acme-challenge/<token>`.
    pub async fn start(
        port: u16,
        answers: std::sync::Arc<HashMap<String, String>>,
    ) -> Result<Self, AcmeError> {
        let listener = TcpListener::bind(("0.0.0.0", port))
            .await
            .map_err(|_| AcmeError::Bind(port))?;
        tracing::info!(port, "acme http-01 challenge server up");
        let handle = tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else { break };
                let answers = answers.clone();
                tokio::spawn(async move {
                    // read request head (bounded)
                    let mut buf = vec![0u8; 2048];
                    let mut req = Vec::new();
                    for _ in 0..4 {
                        let n = sock.read(&mut buf).await.unwrap_or(0);
                        if n == 0 {
                            break;
                        }
                        req.extend_from_slice(&buf[..n]);
                        if req.windows(4).any(|w| w == b"\r\n\r\n") {
                            break;
                        }
                    }
                    let head = String::from_utf8_lossy(&req);
                    let token = head
                        .split_whitespace()
                        .nth(1)
                        .unwrap_or("")
                        .trim_start_matches("/.well-known/acme-challenge/")
                        .trim_end_matches(" HTTP/1.1")
                        .to_string();
                    if let Some(key_auth) = answers.get(&token) {
                        let body = key_auth.clone();
                        let resp = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        );
                        sock.write_all(resp.as_bytes()).await.ok();
                    } else {
                        let _ = sock
                            .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                            .await;
                    }
                });
            }
        });
        Ok(Self { handle })
    }

    pub fn shutdown(&self) {
        self.handle.abort();
    }
}

/// Run a full HTTP-01 issuance for `cfg.domains`.
/// Returns (certificate PEM chain, private key PEM).
pub async fn obtain_certificate(cfg: &Acme) -> Result<(String, String), AcmeError> {
    if cfg.domains.is_empty() {
        return Err(AcmeError::Protocol("no domains configured".into()));
    }

    // Account: restore persisted credentials when available, else create and
    // persist them next to the key file. Avoids LE account rate limits.
    let creds_path = format!("{}.account.json", cfg.key_file);
    let (account, creds_json) = match tokio::fs::read_to_string(&creds_path).await {
        Ok(raw) => match serde_json::from_str::<instant_acme::AccountCredentials>(&raw) {
            Ok(creds) => match Account::from_credentials(creds).await {
                Ok(acc) => {
                    tracing::info!("acme account restored from {}", creds_path);
                    (acc, raw)
                }
                Err(e) => {
                    tracing::warn!("stored acme credentials unusable ({e}); creating new account");
                    create_account(cfg).await?
                }
            },
            Err(e) => {
                tracing::warn!("stored acme credentials corrupt ({e}); creating new account");
                create_account(cfg).await?
            }
        },
        Err(_) => create_account(cfg).await?,
    };
    // keep the credentials file in sync with whatever we ended up using
    let existing = tokio::fs::read_to_string(&creds_path).await.ok();
    if existing.as_deref() != Some(creds_json.as_str()) {
        if let Some(parent) = std::path::Path::new(&creds_path).parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }
        tokio::fs::write(&creds_path, &creds_json).await?;
    }

    let identifiers: Vec<Identifier> =
        cfg.domains.iter().map(|d| Identifier::Dns(d.clone())).collect();
    let mut order = account.new_order(&NewOrder { identifiers: &identifiers }).await?;

    // collect challenges: HTTP-01 (serve locally) or DNS-01 (Cloudflare TXT)
    let authorizations = order.authorizations().await?;
    let mut answers: HashMap<String, String> = HashMap::new();
    let mut created_records: Vec<(String, String)> = Vec::new();
    let mut challenge_urls: Vec<String> = Vec::new();
    for auth in &authorizations {
        let Identifier::Dns(domain) = &auth.identifier;
        for ch in &auth.challenges {
            let want = if cfg.dns01 { ChallengeType::Dns01 } else { ChallengeType::Http01 };
            if ch.r#type != want {
                continue;
            }
            challenge_urls.push(ch.url.clone());
            let key_auth = order.key_authorization(ch).as_str().to_string();
            if cfg.dns01 {
                let Some(token) = &cfg.cloudflare_api_token else {
                    return Err(AcmeError::Protocol("dns01 requires cloudflare_api_token".into()));
                };
                if token.is_empty() {
                    return Err(AcmeError::Protocol("dns01 cloudflare_api_token is empty".into()));
                }
                tracing::info!(domain=%domain, "creating _acme-challenge TXT record");
                let id = crate::cloudflare_txt_create(
                    token,
                    domain,
                    &crate::txt_record_name(domain),
                    &crate::txt_value(&key_auth),
                ).await?;
                created_records.push((domain.clone(), id));
            } else {
                answers.insert(ch.token.clone(), key_auth);
            }
        }
    }
    if cfg.dns01 && created_records.is_empty() {
        return Err(AcmeError::Protocol("no DNS-01 challenges offered".into()));
    }
    if !cfg.dns01 && answers.is_empty() {
        return Err(AcmeError::Protocol("no HTTP-01 challenges offered".into()));
    }

    let server = if cfg.dns01 {
        None
    } else {
        Some(ChallengeServer::start(cfg.http01_port, Arc::new(answers)).await?)
    };
    // DNS-01 TXT records created above; delete them on every exit path
    struct CleanupDns<'a>(&'a Acme, &'a [(String, String)]);
    impl Drop for CleanupDns<'_> {
        fn drop(&mut self) {
            if self.0.dns01 {
                if let Some(token) = &self.0.cloudflare_api_token {
                    for (domain, id) in self.1 {
                        let token = token.clone();
                        let domain = domain.clone();
                        let id = id.clone();
                        tokio::spawn(async move {
                            if let Err(e) = crate::cloudflare_txt_delete(&token, &domain, &id).await {
                                tracing::warn!("failed to delete TXT record {id}: {e}");
                            }
                        });
                    }
                }
            }
        }
    }
    let _cleanup_dns = CleanupDns(&cfg, &created_records);
    for url in &challenge_urls {
        order.set_challenge_ready(url).await?;
    }

    // wait for order ready
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(120);
    loop {
        order.refresh().await?;
        match order.state().status {
            OrderStatus::Ready | OrderStatus::Valid => break,
            OrderStatus::Pending => {}
            ref s => {
                if let Some(srv) = &server { srv.shutdown(); }
                return Err(AcmeError::Protocol(format!("order entered state {s:?}")));
            }
        }
        if tokio::time::Instant::now() > deadline {
            if let Some(srv) = &server { srv.shutdown(); }
            return Err(AcmeError::Timeout);
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }

    // CSR + finalize
    let key = rcgen::KeyPair::generate()?;
    let mut params = rcgen::CertificateParams::default();
    params.distinguished_name = rcgen::DistinguishedName::new();
    params.subject_alt_names = cfg
        .domains
        .iter()
        .map(|d| rcgen::SanType::DnsName(d.clone().try_into().expect("ia5 domain")))
        .collect();
    let cert = params.self_signed(&key)?;
    order.finalize(cert.der()).await?;

    // wait for certificate
    let pem = loop {
        order.refresh().await?;
        match order.certificate().await? {
            Some(p) => break p,
            None => {
                if tokio::time::Instant::now() > deadline {
                    if let Some(srv) = &server { srv.shutdown(); }
                    return Err(AcmeError::Timeout);
                }
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
        }
    };

    if let Some(srv) = &server { srv.shutdown(); }
    Ok((pem, key.serialize_pem()))
}

async fn create_account(cfg: &Acme) -> Result<(Account, String), AcmeError> {
    let (account, creds) = Account::create(
        &NewAccount {
            contact: &[Box::leak(format!("mailto:{}", cfg.email).into_boxed_str())],
            terms_of_service_agreed: true,
            only_return_existing: false,
        },
        &cfg.directory_url,
        None,
    )
    .await?;
    let json = serde_json::to_string(&creds).map_err(|e| AcmeError::Protocol(e.to_string()))?;
    Ok((account, json))
}
