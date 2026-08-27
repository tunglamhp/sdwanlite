//! sdwanlite daemon library: HTTP server wiring and auth policy helpers.

pub mod server;

/// Reject non-loopback API binds when no dashboard auth is configured.
/// The daemon refuses to expose an unauthenticated control API.
pub fn validate_bind_auth(api_addr: &str, auth_env_set: bool) -> Result<(), String> {
    if auth_env_set {
        return Ok(());
    }
    let loopback = api_addr == "127.0.0.1" || api_addr == "::1" || api_addr == "localhost";
    if loopback {
        Ok(())
    } else {
        Err(format!(
            "api_addr = {api_addr} (non-loopback) but SDWANLITE_AUTH_USER/PASS are not set; \
             refusing to expose an unauthenticated control API"
        ))
    }
}
