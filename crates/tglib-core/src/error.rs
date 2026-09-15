use thiserror::Error;

/// Domain-level Telegram errors. Do not include secrets in `message`.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum TelegramError {
    #[error("account not found")]
    AccountNotFound,
    #[error("account is not ready")]
    NotReady,
    #[error("permission denied: {0}")]
    PermissionDenied(&'static str),
    #[error("authorization required")]
    AuthorizationRequired,
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    #[error("rate limited")]
    RateLimited,
    #[error("telegram restricted this action")]
    Restricted,
    #[error("client unavailable: {0}")]
    Unavailable(String),
    #[error("account error: {0}")]
    Account(String),
}

impl TelegramError {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::AccountNotFound => "account_not_found",
            Self::NotReady => "not_ready",
            Self::PermissionDenied(_) => "permission_denied",
            Self::AuthorizationRequired => "authorization_required",
            Self::InvalidRequest(_) => "invalid_request",
            Self::RateLimited => "rate_limited",
            Self::Restricted => "restricted",
            Self::Unavailable(_) => "unavailable",
            Self::Account(_) => "account_error",
        }
    }
}
