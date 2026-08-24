use thiserror::Error;

#[derive(Debug, Error)]
pub enum AcmeError {
    #[error("ACME protocol error: {0}")]
    Protocol(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("key/cert generation error: {0}")]
    Rcgen(String),
    #[error("challenge server port {0} could not be bound")]
    Bind(u16),
    #[error("order did not become ready in time")]
    Timeout,
}

impl From<instant_acme::Error> for AcmeError {
    fn from(e: instant_acme::Error) -> Self {
        Self::Protocol(e.to_string())
    }
}

impl From<rcgen::Error> for AcmeError {
    fn from(e: rcgen::Error) -> Self {
        Self::Rcgen(e.to_string())
    }
}
