//! HTTP/2 upstream support: bridge an HTTP/1.x client request onto an
//! h2 backend connection (buffered-body bridging, lab grade).

use std::net::SocketAddr;
use tokio::io::AsyncWriteExt;
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

/// A live h2 client session to one backend.
pub struct H2Session {
    pub send: h2::client::SendRequest<bytes::Bytes>,
}

/// Establish an h2 client session to `addr`.
pub async fn open_session(addr: SocketAddr) -> std::io::Result<H2Session> {
    let tcp = TcpStream::connect(addr).await?;
    let (send_request, connection) = h2::client::handshake(tcp)
        .await
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("h2 handshake: {e}")))?;
    tokio::spawn(async move {
        if let Err(e) = connection.await {
            tracing::debug!("h2 connection ended: {e}");
        }
    });
    let send = send_request
        .ready()
        .await
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("h2 not ready: {e}")))?;
    Ok(H2Session { send })
}

impl H2Session {
    /// Send request headers (stream not ended).
    pub fn begin(
        &mut self,
        head: &HeadParts,
        has_body: bool,
    ) -> std::io::Result<(
        h2::client::ResponseFuture,
        h2::SendStream<bytes::Bytes>,
    )> {
        let mut builder = http::Request::builder()
            .method(head.method.as_str())
            .uri(head.path.as_str());
        builder = builder.header(
            http::header::HeaderName::from_static("host"),
            if head.authority.is_empty() { "localhost".into() } else { head.authority.clone() },
        );
        for (n, v) in &head.headers {
            if n.eq_ignore_ascii_case("content-length") && !has_body {
                continue;
            }
            builder = builder.header(n.as_str(), v.as_str());
        }
        if has_body {
            builder = builder.header(http::header::CONTENT_LENGTH, "0");
        }
        let request = match builder.body(()) {
            Ok(r) => r,
            Err(e) => return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())),
        };
        self.send
            .send_request(request, false)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("h2 send: {e}")))
    }
}

/// Stream all of `data` through an h2 send stream honoring flow control.
pub async fn send_all(send: &mut h2::SendStream<bytes::Bytes>, mut data: &[u8]) -> std::io::Result<()> {
    while !data.is_empty() {
        send.reserve_capacity(data.len());
        let cap = std::future::poll_fn(|cx| send.poll_capacity(cx))
            .await
            .transpose()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("h2 capacity: {e}")))?
            .unwrap_or(0);
        if cap == 0 {
            return Err(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "h2 stream closed"));
        }
        let n = cap.min(data.len());
        let _ = send.send_data(bytes::Bytes::copy_from_slice(&data[..n]), false);
        data = &data[n..];
    }
    Ok(())
}

/// Write a chunked-transfer block.
pub async fn write_chunk<W: tokio::io::AsyncWrite + Unpin>(
    w: &mut W,
    data: &[u8],
) -> std::io::Result<()> {
    if data.is_empty() {
        return Ok(());
    }
    w.write_all(format!("{:x}\r\n", data.len()).as_bytes()).await?;
    w.write_all(data).await?;
    w.write_all(b"\r\n").await
}

