//! API auth integration tests: dashboard Basic gate, mutation-endpoint
//! authorization (Basic browser path + Bearer script path), and the
//! non-loopback hard-fail policy. Tests that touch process env are
//! serialized with a static mutex because env vars are process-global.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::middleware::from_fn;
use tower::ServiceExt;

use sdwanlite_app::server::{self, AppState};
use sdwanlite_core::{Config, PathPolicyStore};
use sdwanlite_lb::AlertLog;

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn app() -> axum::Router {
    let state = Arc::new(AppState {
        config: Arc::new(Config::sample()),
        started: Instant::now(),
        tcp_pools: vec![],
        http_pools: vec![],
        bgp: None,
        alerts: AlertLog::new(100).into(),
        path_policy: Mutex::new(PathPolicyStore::default()),
        path_policy_path: std::env::temp_dir().join("sdwanlite-test-path-policy.json"),
        pool_overrides: Mutex::new(HashMap::new()),
        pool_overrides_path: std::env::temp_dir().join("sdwanlite-test-overrides.json"),
    });
    server::router(state).layer(from_fn(server::auth_middleware))
}

async fn get(app: &axum::Router, uri: &str, auth: Option<&str>) -> StatusCode {
    let mut rb = Request::builder().uri(uri);
    if let Some(h) = auth {
        rb = rb.header("authorization", h);
    }
    app.clone()
        .oneshot(rb.body(Body::empty()).unwrap())
        .await
        .unwrap()
        .status()
}

#[tokio::test]
async fn dashboard_api_rejects_unauthenticated() {
    let _g = ENV_LOCK.lock().unwrap();
    std::env::set_var("SDWANLITE_AUTH_USER", "admin");
    std::env::set_var("SDWANLITE_AUTH_PASS", "s3cret");
    std::env::set_var("SDWANLITE_API_TOKEN", "tok");
    let app = app();
    assert_eq!(
        get(&app, "/api/status", None).await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        get(&app, "/api/labels", None).await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        get(&app, "/api/status", Some("Basic dXNlcjpub3B3")).await,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn dashboard_api_accepts_valid_basic() {
    let _g = ENV_LOCK.lock().unwrap();
    std::env::set_var("SDWANLITE_AUTH_USER", "admin");
    std::env::set_var("SDWANLITE_AUTH_PASS", "s3cret");
    std::env::set_var("SDWANLITE_API_TOKEN", "tok");
    let app = app();
    assert_eq!(
        get(&app, "/api/status", Some("Basic YWRtaW46czNjcmV0")).await,
        StatusCode::OK
    );
    assert_eq!(
        get(&app, "/api/labels", Some("Basic YWRtaW46czNjcmV0")).await,
        StatusCode::OK
    );
}

#[tokio::test]
async fn mutation_endpoints_accept_browser_basic_auth() {
    let _g = ENV_LOCK.lock().unwrap();
    std::env::set_var("SDWANLITE_AUTH_USER", "admin");
    std::env::set_var("SDWANLITE_AUTH_PASS", "s3cret");
    std::env::set_var("SDWANLITE_API_TOKEN", "tok");
    let app = app();
    // firewall add: config-managed message, but must pass authorization
    let rsp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/firewall")
                .header("authorization", "Basic YWRtaW46czNjcmV0")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"action":"drop"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(rsp.status(), StatusCode::OK);
    // same request without credentials is rejected by the middleware
    let rsp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/firewall")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"action":"drop"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(rsp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn mutation_endpoints_accept_bearer_when_basic_gate_absent() {
    let _g = ENV_LOCK.lock().unwrap();
    // Dev mode: no Basic credentials -> middleware passes everything,
    // the mutation endpoints still require the bearer token.
    std::env::remove_var("SDWANLITE_AUTH_USER");
    std::env::remove_var("SDWANLITE_AUTH_PASS");
    std::env::set_var("SDWANLITE_API_TOKEN", "tok");
    let app = app();

    let ok = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/firewall")
                .header("authorization", "Bearer tok")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"action":"drop"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(ok.status(), StatusCode::OK);

    let denied = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/firewall")
                .header("authorization", "Bearer wrong")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"action":"drop"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::OK); // passes middleware in dev mode
    let body = axum::body::to_bytes(denied.into_body(), 4096)
        .await
        .unwrap();
    assert!(String::from_utf8_lossy(&body).contains("unauthorized"));
}

#[test]
fn non_loopback_bind_requires_auth_env() {
    // Hard-fail: non-loopback + no auth -> Err
    assert!(sdwanlite_app::validate_bind_auth("0.0.0.0", false).is_err());
    assert!(sdwanlite_app::validate_bind_auth("192.168.1.5", false).is_err());
    // Loopback binds are fine in dev mode
    assert!(sdwanlite_app::validate_bind_auth("127.0.0.1", false).is_ok());
    assert!(sdwanlite_app::validate_bind_auth("::1", false).is_ok());
    assert!(sdwanlite_app::validate_bind_auth("localhost", false).is_ok());
    // Auth configured -> any bind is allowed
    assert!(sdwanlite_app::validate_bind_auth("0.0.0.0", true).is_ok());
    assert!(sdwanlite_app::validate_bind_auth("192.168.1.5", true).is_ok());
}
