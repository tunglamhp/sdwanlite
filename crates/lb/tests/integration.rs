//! Integration-style tests for TLS termination and connection limits.

use sdwanlite_lb::tcp::TcpLoadBalancer;
use sdwanlite_lb::HttpLoadBalancer;
use sdwanlite_core::{Algorithm, HttpPool, HttpRoute, TcpPool, TlsConfig};
use std::sync::Arc;
use futures_core::Stream as _;

fn gen_self_signed(dir: &std::path::Path) -> (String, String) {
    // Use rcgen to mint a self-signed cert/key pair.
    let key = rcgen::KeyPair::generate().expect("key");
    let mut params = rcgen::CertificateParams::default();
    params.subject_alt_names = vec![
        rcgen::SanType::DnsName("localhost".try_into().expect("ia5")),
        rcgen::SanType::IpAddress(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)),
    ];
    let cert = params.self_signed(&key).expect("self signed");
    let cert_path = dir.join("cert.pem");
    let key_path = dir.join("key.pem");
    std::fs::write(&cert_path, cert.pem()).unwrap();
    std::fs::write(&key_path, key.serialize_pem()).unwrap();
    (
        cert_path.to_string_lossy().into_owned(),
        key_path.to_string_lossy().into_owned(),
    )
}

fn rustls_client_root(pem: &str) -> Arc<rustls::ClientConfig> {
    let mut roots = rustls::RootCertStore::empty();
    let cert = rustls_pemfile::certs(&mut pem.as_bytes())
        .next()
        .unwrap()
        .unwrap();
    roots.add(cert).unwrap();
    Arc::new(
        rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth(),
    )
}

#[tokio::test]
async fn tls_handshake_through_http_lb() {
    let dir = tempfile::tempdir().unwrap();
    let (cert, key) = gen_self_signed(dir.path());

    // dummy backend that accepts and immediately closes
    let backend = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let baddr = backend.local_addr().unwrap().to_string();
    tokio::spawn(async move {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let resp = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nhi";
        while let Ok((mut s, _)) = backend.accept().await {
            let mut buf = [0u8; 1024];
            let _ = tokio::time::timeout(std::time::Duration::from_secs(2), s.read(&mut buf)).await;
            let _ = s.write_all(resp).await;
            // drop: closes
        }
    });

    let addr_port = baddr;
    let pool = HttpPool {
        name: "tls-test".into(),
        listen: "127.0.0.1:0".into(),
        backend_proto: sdwanlite_core::BackendProto::Http1,
        tls: Some(TlsConfig {
            cert_file: cert.clone(),
            key_file: key.clone(),
        }),
        health_check_path: None,
        health_interval_secs: 5,
        health_timeout_secs: 2,
        routes: vec![HttpRoute {
            host: String::new(),
            path_prefix: "/".into(),
            backends: vec![addr_port],
        }],
    };
    let lb = HttpLoadBalancer::bind(&pool).await.unwrap();
    let server = tokio::spawn({
        let lb = lb.clone();
        async move { lb.serve().await }
    });

    let local = lb.local_addr().unwrap();

    // TLS client
    let client_cfg = rustls_client_root(
        &std::fs::read_to_string(&cert).unwrap(),
    );
    let connector = tokio_rustls::TlsConnector::from(client_cfg);
    let sock = tokio::net::TcpStream::connect(local).await.unwrap();
    let mut tls = connector
        .connect("localhost".try_into().unwrap(), sock)
        .await
        .expect("tls handshake");

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    tls.write_all(format!("GET / HTTP/1.1\r\nHost: localhost\r\n\r\n").as_bytes())
        .await
        .unwrap();
    // Read response until EOF (backend closes immediately).
    let mut got = Vec::new();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        assert!(std::time::Instant::now() < deadline, "timeout waiting for response");
        let mut buf = vec![0u8; 256];
        match tokio::time::timeout(std::time::Duration::from_millis(500), tls.read(&mut buf)).await {
            Err(_) => continue,
            Ok(Err(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Ok(Err(e)) => panic!("read error: {e}"),
            Ok(Ok(0)) => break,
            Ok(Ok(n)) => got.extend_from_slice(&buf[..n]),
        }
    }
    let head = String::from_utf8_lossy(&got);
    assert!(head.starts_with("HTTP/1.1"), "got: {head}");

    lb.stop();
    server.abort();
}

#[tokio::test]
async fn tcp_conn_limit_rejects() {
    // backend echo server
    let bl = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let baddr = bl.local_addr().unwrap().to_string();
    tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = bl.accept().await else { break };
            tokio::spawn(async move {
                let mut buf = [0u8; 64];
                while let Ok(n) = s.read(&mut buf).await {
                    if n == 0 { break }
                    s.write_all(&buf[..n]).await.ok();
                }
            });
        }
    });

    let pool = TcpPool {
        name: "limit-test".into(),
        listen: "127.0.0.1:0".into(),
        algorithm: Algorithm::RoundRobin,
        health_check_path: None,
        health_interval_secs: 60, // don't interfere
        health_timeout_secs: 2,
        backends: vec![baddr],
    };
    let lb = TcpLoadBalancer::bind(&pool).await.unwrap();
    lb.set_max_conns(1);
    let local = lb.local_addr().unwrap();
    let server = tokio::spawn({
        let lb = lb.clone();
        async move { lb.serve().await }
    });

    let c1 = tokio::net::TcpStream::connect(local).await.unwrap();
    // wait until the accept loop registers the connection
    for _ in 0..50 {
        if lb.active_conns() == 1 { break }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(lb.active_conns(), 1);

    // second conn should be rejected (closed immediately by the LB)
    let c2 = tokio::net::TcpStream::connect(local).await.unwrap();
    for _ in 0..50 {
        if lb.rejected_conns() == 1 { break }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(lb.rejected_conns(), 1);
    drop(c2);

    // first connection still works: echo roundtrip
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut c1 = c1;
    c1.write_all(b"ping").await.unwrap();
    let mut buf = [0u8; 4];
    tokio::time::timeout(std::time::Duration::from_secs(2), c1.read_exact(&mut buf))
        .await
        .expect("echo within timeout")
        .unwrap();
    assert_eq!(&buf, b"ping");

    lb.stop();
    server.abort();
}

#[tokio::test]
async fn h2_upstream_roundtrip() {
    // real h2 backend
    let bl = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let baddr = bl.local_addr().unwrap().to_string();
    tokio::spawn(async move {
        while let Ok((sock, _)) = bl.accept().await {
            let mut conn = match h2::server::handshake(sock).await {
                Ok(c) => c,
                Err(_) => continue,
            };
            tokio::spawn(async move {
                while let Some(Ok((req, mut respond))) = conn.accept().await {
                    let path = req.uri().path().to_string();
                    tokio::spawn(async move {
                        let body = format!("hello-h2 {path}");
                        let resp = http::Response::builder()
                            .status(200)
                            .body(())
                            .unwrap();
                        if let Ok(mut tx) = respond.send_response(resp, false) {
                            let _ = tx.send_data(body.into_bytes().into(), true);
                        }
                    });
                }
            });
        }
    });

    let pool = HttpPool {
        name: "h2-test".into(),
        listen: "127.0.0.1:0".into(),
        tls: None,
        health_check_path: None,
        backend_proto: sdwanlite_core::BackendProto::H2,
        health_interval_secs: 60,
        health_timeout_secs: 2,
        routes: vec![HttpRoute {
            host: String::new(),
            path_prefix: "/".into(),
            backends: vec![baddr],
        }],
    };
    let lb = HttpLoadBalancer::bind(&pool).await.unwrap();
    let local = lb.local_addr().unwrap();
    let server = tokio::spawn({
        let lb = lb.clone();
        async move { lb.serve().await }
    });

    // plain HTTP/1.1 client -> LB -> h2 backend
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut c = tokio::net::TcpStream::connect(local).await.unwrap();
    c.write_all(b"GET /world HTTP/1.1\r\nHost: t\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut got = Vec::new();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        assert!(std::time::Instant::now() < deadline, "timeout");
        let mut buf = vec![0u8; 512];
        match tokio::time::timeout(std::time::Duration::from_millis(500), c.read(&mut buf)).await {
            Err(_) => continue,
            Ok(Ok(0)) | Ok(Err(_)) => break,
            Ok(Ok(n)) => got.extend_from_slice(&buf[..n]),
        }
    }
    let text = String::from_utf8_lossy(&got);
    assert!(text.contains("200"), "expected 200, got: {text}");
    assert!(text.contains("hello-h2 /world"), "got: {text}");

    lb.stop();
    server.abort();
}
