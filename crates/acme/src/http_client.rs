//! HTTP client for instant-acme that injects `wildcard: true` into DNS
//! identifiers of new-order requests — the escape hatch needed because the
//! library's typed `Identifier` cannot express ACME wildcards.

use bytes::Bytes;
use http::{Request, Request as _RequestAlias};
use http_body_util::Full;
use hyper_rustls::HttpsConnector;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;
use instant_acme::{BytesResponse, Error, HttpClient};
use std::future::Future;
use std::pin::Pin;

type HyperClient = hyper_util::client::legacy::Client<HttpsConnector<HttpConnector>, Full<Bytes>>;

/// Delegating client that marks every DNS identifier in a new-order payload
/// as a wildcard when `all_wildcards` is set.
pub struct WildcardHttpClient {
    inner: HyperClient,
    pub all_wildcards: bool,
}

impl WildcardHttpClient {
    pub fn try_new(all_wildcards: bool) -> Result<Self, Error> {
        let inner = hyper_util::client::legacy::Client::builder(TokioExecutor::new()).build(
            hyper_rustls::HttpsConnectorBuilder::new()
                .with_native_roots()
                .map_err(|e| Error::Other(Box::new(e)))?
                .https_only()
                .enable_http1()
                .enable_http2()
                .build(),
        );
        Ok(Self {
            inner,
            all_wildcards,
        })
    }
}

impl HttpClient for WildcardHttpClient {
    fn request(
        &self,
        mut req: _RequestAlias<Full<Bytes>>,
    ) -> Pin<Box<dyn Future<Output = Result<BytesResponse, Error>> + Send>> {
        let inner = self.inner.clone();
        let all_wildcards = self.all_wildcards;
        Box::pin(async move {
            if all_wildcards {
                use http_body_util::BodyExt as _;
                let (mut parts, body) = req.into_parts();
                let collected = match body.collect().await {
                    Ok(c) => c,
                    Err(e) => return Err(Error::Other(Box::new(e))),
                };
                let data = collected.to_bytes();
                if let Ok(mut json) = serde_json::from_slice::<serde_json::Value>(&data) {
                    let mut changed = false;
                    if let Some(ids) = json["identifiers"].as_array_mut() {
                        for id in ids {
                            if id["type"] == "dns" && id.get("wildcard").is_none() {
                                id["wildcard"] = serde_json::Value::Bool(true);
                                changed = true;
                            }
                        }
                    }
                    let payload = if changed {
                        serde_json::to_vec(&json).map_err(|e| Error::Other(Box::new(e)))?
                    } else {
                        data.to_vec()
                    };
                    parts
                        .headers
                        .insert(http::header::CONTENT_LENGTH, payload.len().into());
                    req = Request::from_parts(parts, Full::new(Bytes::from(payload)));
                } else {
                    // not JSON: rebuild unchanged
                    req = Request::from_parts(parts, Full::new(data));
                }
            }
            match inner.request(req).await {
                Ok(rsp) => Ok(BytesResponse::from(rsp)),
                Err(e) => Err(e.into()),
            }
        })
    }
}
