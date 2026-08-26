//! Pluggable DNS-01 providers. Implement [`DnsProvider`] to add a new one.

use crate::error::AcmeError;
use async_trait::async_trait;

/// One DNS-01 challenge to materialize as a TXT record.
pub struct TxtChallenge {
    /// FQDN of the record, e.g. `_acme-challenge.example.com`.
    pub name: String,
    /// Base64url(SHA-256(key_authorization)).
    pub value: String,
}

#[async_trait]
pub trait DnsProvider: Send + Sync {
    fn name(&self) -> &'static str;
    async fn create_txt(&self, ch: &TxtChallenge) -> Result<String, AcmeError>;
    async fn delete_txt(&self, record_id: &str) -> Result<(), AcmeError>;
}

// ---------------------------------------------------------------------------
// Cloudflare
// ---------------------------------------------------------------------------

pub struct CloudflareProvider {
    pub api_token: String,
    /// Registrable domain used for zone lookup (e.g. `example.com`).
    pub zone: String,
}

#[async_trait]
impl DnsProvider for CloudflareProvider {
    fn name(&self) -> &'static str {
        "cloudflare"
    }

    async fn create_txt(&self, ch: &TxtChallenge) -> Result<String, AcmeError> {
        let id =
            crate::dns01::cloudflare_txt_create(&self.api_token, &self.zone, &ch.name, &ch.value)
                .await?;
        Ok(id)
    }

    async fn delete_txt(&self, record_id: &str) -> Result<(), AcmeError> {
        crate::dns01::cloudflare_txt_delete(&self.api_token, &self.zone, record_id).await
    }
}

// ---------------------------------------------------------------------------
// DigitalOcean
// ---------------------------------------------------------------------------

pub struct DigitalOceanProvider {
    pub api_token: String,
    /// Bare domain managed by the account (e.g. `example.com`).
    pub domain: String,
}

const DO_API: &str = "https://api.digitalocean.com/v2";

fn do_request(
    token: &str,
    method: reqwest::Method,
    url: String,
) -> Result<reqwest::RequestBuilder, AcmeError> {
    Ok(reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| AcmeError::Protocol(e.to_string()))?
        .request(method, url)
        .bearer_auth(token))
}

/// DO stores the full record name including the domain suffix.
fn do_full_name(ch: &TxtChallenge, domain: &str) -> String {
    let base = format!(".{domain}");
    if ch.name.ends_with(&base) {
        ch.name.clone()
    } else {
        format!("{}.{}", ch.name, domain)
    }
}

#[async_trait]
impl DnsProvider for DigitalOceanProvider {
    fn name(&self) -> &'static str {
        "digitalocean"
    }

    async fn create_txt(&self, ch: &TxtChallenge) -> Result<String, AcmeError> {
        let http = do_request(
            &self.api_token,
            reqwest::Method::POST,
            format!("{DO_API}/domains/{}/records", self.domain),
        )?;
        let rsp = http
            .json(&serde_json::json!({
                "type": "TXT",
                "name": do_full_name(ch, &self.domain),
                "data": ch.value,
                "ttl": 60,
            }))
            .send()
            .await
            .map_err(|e| AcmeError::Protocol(e.to_string()))?;
        let status = rsp.status();
        let json: serde_json::Value = rsp
            .json()
            .await
            .map_err(|e| AcmeError::Protocol(e.to_string()))?;
        if !status.is_success() {
            return Err(AcmeError::Protocol(format!(
                "digitalocean ({status}): {json}"
            )));
        }
        json["record"]["id"]
            .as_u64()
            .map(|id| id.to_string())
            .ok_or_else(|| AcmeError::Protocol("no record id".into()))
    }

    async fn delete_txt(&self, record_id: &str) -> Result<(), AcmeError> {
        let rsp = do_request(
            &self.api_token,
            reqwest::Method::DELETE,
            format!("{DO_API}/domains/{}/records/{}", self.domain, record_id),
        )?
        .send()
        .await
        .map_err(|e| AcmeError::Protocol(e.to_string()))?;
        if !rsp.status().is_success() {
            return Err(AcmeError::Protocol(format!(
                "digitalocean delete failed: {}",
                rsp.status()
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn do_full_name_variants() {
        let mk = |name: &str| TxtChallenge {
            name: name.into(),
            value: "v".into(),
        };
        assert_eq!(
            do_full_name(&mk("_acme-challenge.example.com"), "example.com"),
            "_acme-challenge.example.com"
        );
        assert_eq!(
            do_full_name(&mk("_acme-challenge"), "example.com"),
            "_acme-challenge.example.com"
        );
    }

    #[test]
    fn provider_names() {
        let cf = CloudflareProvider {
            api_token: "t".into(),
            zone: "example.com".into(),
        };
        let d_o = DigitalOceanProvider {
            api_token: "t".into(),
            domain: "example.com".into(),
        };
        assert_eq!(cf.name(), "cloudflare");
        assert_eq!(d_o.name(), "digitalocean");
    }
}
