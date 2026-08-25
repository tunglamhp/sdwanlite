//! DNS-01 helpers: TXT record naming, value derivation, and a minimal
//! Cloudflare DNS API client (Zone.DNS Edit token).

use crate::error::AcmeError;
use base64::Engine as _;
use sha2::{Digest, Sha256};
use std::time::Duration;

/// TXT record name for a challenge: `_acme-challenge.<domain>`.
pub fn txt_record_name(domain: &str) -> String {
    format!("_acme-challenge.{domain}")
}

/// Base64url(SHA-256(key_authorization)) — the TXT record value (RFC 8555 §8.4).
pub fn txt_value(key_authorization: &str) -> String {
    let digest = Sha256::digest(key_authorization.as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest)
}

/// Strip a leading wildcard and reduce to the zone apex guess
/// (`*.a.b.example.com` -> `example.com`). Good enough for the common case;
/// explicit zones with deeper splits should use a delegated token scoped to
/// the exact zone.
pub fn zone_apex(domain: &str) -> String {
    let d = domain.trim_start_matches("*.");
    let labels: Vec<&str> = d.split('.').filter(|l| !l.is_empty()).collect();
    if labels.len() >= 2 {
        labels[labels.len() - 2..].join(".")
    } else {
        d.to_string()
    }
}

#[allow(dead_code)]
const CF_API: &str = "https://api.cloudflare.com/client/v4";

fn client(token: &str) -> Result<reqwest::Client, AcmeError> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| AcmeError::Protocol(format!("http client: {e}")))
        .map(|c| {
            c
        })
        .map(|c| {
            // attach auth per-request instead; return plain client
            c
        })
        .map(|c| {
            let _ = token;
            c
        })
}

async fn cf_request(
    method: reqwest::Method,
    url: String,
    token: &str,
    body: Option<serde_json::Value>,
) -> Result<serde_json::Value, AcmeError> {
    let http = match body {
        Some(json) => client(token)?
            .request(method, url)
            .bearer_auth(token)
            .json(&json),
        None => client(token)?.request(method, url).bearer_auth(token),
    };
    let rsp = http.send().await.map_err(|e| AcmeError::Protocol(e.to_string()))?;
    let status = rsp.status();
    let json: serde_json::Value =
        rsp.json().await.map_err(|e| AcmeError::Protocol(e.to_string()))?;
    if !status.is_success() || json["success"].as_bool() != Some(true) {
        return Err(AcmeError::Protocol(format!(
            "cloudflare api error ({status}): {}",
            json["errors"]
        )));
    }
    Ok(json)
}

/// Create a TXT record for `name` in the zone covering `domain`.
/// Returns the record id for later deletion.
pub async fn cloudflare_txt_create(
    token: &str,
    domain: &str,
    name: &str,
    value: &str,
) -> Result<String, AcmeError> {
    let zone = zone_apex(domain);
    let zones = cf_request(
        reqwest::Method::GET,
        format!("{CF_API}/zones?name={zone}"),
        token,
        None,
    )
    .await?;
    let zone_id = zones["result"][0]["id"]
        .as_str()
        .ok_or_else(|| AcmeError::Protocol(format!("zone '{zone}' not found or not accessible")))?
        .to_string();

    let created = cf_request(
        reqwest::Method::POST,
        format!("{CF_API}/zones/{zone_id}/dns_records"),
        token,
        Some(serde_json::json!({
            "type": "TXT",
            "name": name,
            "content": value,
            "ttl": 60,
        })),
    )
    .await?;
    created["result"]["id"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| AcmeError::Protocol("no record id returned".into()))
}

/// Delete a previously created TXT record.
pub async fn cloudflare_txt_delete(
    token: &str,
    domain: &str,
    record_id: &str,
) -> Result<(), AcmeError> {
    let zone = zone_apex(domain);
    cf_request(
        reqwest::Method::DELETE,
        format!("{CF_API}/zones/{zone}/dns_records/{record_id}"),
        token,
        None,
    )
    .await?;
    Ok(())
}
