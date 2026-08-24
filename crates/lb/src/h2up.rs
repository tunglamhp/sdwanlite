//! HTTP/2 upstream support: bridge an HTTP/1.x client request onto an
//! h2 backend connection (buffered-body bridging, lab grade).

use std::net::SocketAddr;
use tokio::net::TcpStream;

/// Parsed request head pieces needed to rebuild the request on h2.
#[derive(Debug, Clone)]
pub struct HeadParts {
    pub method: String,
    pub path: String,
    pub authority: String,
    pub headers: Vec<(String, String)>,
}

/// Parse "METHOD SP PATH SP VERSION" plus headers into HeadParts.
/// Returns None on malformed input.
pub fn parse_head(head: &str) -> Option<HeadParts> {
    let mut lines = head.split("\r\n");
    let req = lines.next()?;
    let mut it = req.split_whitespace();
    let method = it.next()?.to_ascii_uppercase();
    let path = it.next()?.to_string();
    if !method.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }

    let mut headers = Vec::new();
    let mut authority = String::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let name = name.trim();
        let value = value.trim();
        match name.to_ascii_lowercase().as_str() {
            "host" if authority.is_empty() => authority = value.to_string(),
            // hop-by-hop / forbidden in h2
            "connection" | "keep-alive" | "proxy-connection" | "transfer-encoding" | "upgrade" => {}
            _ => headers.push((name.to_string(), value.to_string())),
        }
    }
    Some(HeadParts { method, path, authority, headers })
}

/// Establish an h2 client session to `addr`.
async fn open_session(addr: SocketAddr) -> std::io::Result<h2::client::SendRequest<bytes::Bytes>> {
    let tcp = TcpStream::connect(addr).await?;
    let (send_request, connection) = h2::client::handshake(tcp)
        .await
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("h2 handshake: {e}")))?;
    tokio::spawn(async move {
        if let Err(e) = connection.await {
            tracing::debug!("h2 connection ended: {e}");
        }
    });
    let send_request = send_request
        .ready()
        .await
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("h2 not ready: {e}")))?;
    Ok(send_request)
}

/// Send one request and collect the full response.
/// Returns (status, headers, body).
pub async fn exchange(
    addr: SocketAddr,
    head: &HeadParts,
    body: bytes::Bytes,
) -> std::io::Result<(u16, Vec<(String, String)>, Vec<u8>)> {
    for _ in 0..2 {
        let mut session = match open_session(addr).await {
            Ok(s) => s,
            Err(e) => return Err(e),
        };

        let mut builder = http::Request::builder()
            .method(head.method.as_str())
            .uri(head.path.as_str());
        builder = builder.header(
            http::header::HeaderName::from_static("host"),
            if head.authority.is_empty() { addr.to_string() } else { head.authority.clone() },
        );
        for (n, v) in &head.headers {
            if n.eq_ignore_ascii_case("content-length") && body.is_empty() {
                continue;
            }
            builder = builder.header(n.as_str(), v.as_str());
        }
        if !body.is_empty() || matches!(head.method.as_str(), "POST" | "PUT" | "PATCH") {
            builder = builder.header(http::header::CONTENT_LENGTH, body.len().to_string());
        }
        let request = match builder.body(()) {
            Ok(r) => r,
            Err(e) => return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())),
        };

        let (response, mut send_stream) = match session.send_request(request, false) {
            Ok(x) => x,
            // session died between ready() and send; retry once
            Err(_) => continue,
        };
        let payload = if body.is_empty() { bytes::Bytes::new() } else { body.clone() };
        let _ = send_stream.send_data(payload, true);

        let resp = response
            .await
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("h2 response: {e}")))?;
        let status = resp.status().as_u16();

        let headers: Vec<(String, String)> = resp
            .headers()
            .iter()
            .filter(|(n, _)| **n != http::header::TRANSFER_ENCODING)
            .map(|(n, v)| (n.as_str().to_string(), String::from_utf8_lossy(v.as_bytes()).into_owned()))
            .collect();

        // collect body
        let mut out = Vec::new();
        let mut stream = resp.into_body();
        loop {
            match futures_util_poll_next(&mut stream).await {
                Some(Ok(chunk)) => {
                    out.extend_from_slice(&chunk);
                    if out.len() > 32 * 1024 * 1024 {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::FileTooLarge,
                            "h2 body exceeds 32MiB",
                        ));
                    }
                }
                Some(Err(e)) => {
                    return Err(std::io::Error::new(std::io::ErrorKind::Other, format!("h2 body: {e}")))
                }
                None => break,
            }
        }
        return Ok((status, headers, out));
    }
    Err(std::io::Error::new(std::io::ErrorKind::Other, "h2 session unavailable"))
}

// tiny helper to avoid pulling futures crate just for StreamExt
async fn futures_util_poll_next(
    body: &mut h2::RecvStream,
) -> Option<Result<bytes::Bytes, h2::Error>> {
    std::future::poll_fn(|cx| body.poll_data(cx)).await
}
