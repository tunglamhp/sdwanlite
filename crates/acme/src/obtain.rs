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

    pub fn shutdown(self) {
        self.handle.abort();
    }
}

/// Run a full HTTP-01 issuance for `cfg.domains`.
/// Returns (certificate PEM chain, private key PEM).
pub async fn obtain_certificate(cfg: &Acme) -> Result<(String, String), AcmeError> {
    if cfg.domains.is_empty() {
        return Err(AcmeError::Protocol("no domains configured".into()));
    }

    // account (lab-grade: created per run; persist credentials for reuse later)
    let (account, _creds) = Account::create(
        &NewAccount {
            contact: &[Box::leak(format!("mailto:{}", cfg.email).into_boxed_str())],
            terms_of_service_agreed: true,
            only_return_existing: false,
        },
        &cfg.directory_url,
        None,
    )
    .await?;

    let identifiers: Vec<Identifier> =
        cfg.domains.iter().map(|d| Identifier::Dns(d.clone())).collect();
    let mut order = account.new_order(&NewOrder { identifiers: &identifiers }).await?;

    // collect HTTP-01 challenges and serve them
    let authorizations = order.authorizations().await?;
    let mut answers: HashMap<String, String> = HashMap::new();
    let mut challenge_urls: Vec<String> = Vec::new();
    for auth in &authorizations {
        for ch in &auth.challenges {
            if ch.r#type == ChallengeType::Http01 {
                let key_auth = order.key_authorization(ch);
                answers.insert(ch.token.clone(), key_auth.as_str().to_string());
                challenge_urls.push(ch.url.clone());
            }
        }
    }
    if answers.is_empty() {
        return Err(AcmeError::Protocol("no HTTP-01 challenges offered".into()));
    }

    let server = ChallengeServer::start(cfg.http01_port, Arc::new(answers)).await?;
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
                server.shutdown();
                return Err(AcmeError::Protocol(format!("order entered state {s:?}")));
            }
        }
        if tokio::time::Instant::now() > deadline {
            server.shutdown();
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
                    server.shutdown();
                    return Err(AcmeError::Timeout);
                }
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
        }
    };

    server.shutdown();
    Ok((pem, key.serialize_pem()))
}
