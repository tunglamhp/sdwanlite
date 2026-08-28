//! Crate-wide error type. Keeps `anyhow` free for binary glue while library callers
//! (controller tests, integration suites) get structured errors via `thiserror`.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use sdwan_core::ConfigVersion;
use serde_json::json;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AgentError {
    #[error("config version mismatch: incoming={incoming} current={current}")]
    ConfigVersion {
        incoming: ConfigVersion,
        current: ConfigVersion,
    },

    #[error("org mismatch: incoming={incoming} current={current}")]
    OrgMismatch { incoming: String, current: String },

    #[error("verify callback failed: {0}")]
    VerifyFailed(String),

    #[error("unauthorized")]
    Unauthorized,

    #[error("device already registered")]
    AlreadyRegistered,

    #[error("not found")]
    NotFound,

    #[error("storage error: {0}")]
    Storage(String),

    #[error("websocket error: {0}")]
    Websocket(String),

    #[error("http error: {0}")]
    Http(String),

    #[error("internal: {0}")]
    Internal(String),
}

impl IntoResponse for AgentError {
    fn into_response(self) -> Response {
        let (status, code) = match &self {
            AgentError::ConfigVersion { .. } => (StatusCode::CONFLICT, "config_version_mismatch"),
            AgentError::OrgMismatch { .. } => (StatusCode::FORBIDDEN, "org_mismatch"),
            AgentError::VerifyFailed(_) => (StatusCode::CONFLICT, "verify_failed"),
            AgentError::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
            AgentError::AlreadyRegistered => (StatusCode::CONFLICT, "already_registered"),
            AgentError::NotFound => (StatusCode::NOT_FOUND, "not_found"),
            AgentError::Storage(_) => (StatusCode::INTERNAL_SERVER_ERROR, "storage_error"),
            AgentError::Websocket(_) => (StatusCode::BAD_GATEWAY, "websocket_error"),
            AgentError::Http(_) => (StatusCode::BAD_GATEWAY, "upstream_error"),
            AgentError::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal_error"),
        };
        // Never echo the full error string — it can contain endpoint IPs / tokens. The
        // structured `code` is what dashboards and scripts should branch on.
        tracing::error!(code, "agent error");
        let body = json!({
            "error": code,
            "message": "see server logs",
        });
        (status, axum::Json(body)).into_response()
    }
}

pub type Result<T> = std::result::Result<T, AgentError>;
