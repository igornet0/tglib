use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::ids::TelegramAccountId;
use crate::permissions::TelegramAccountPermissions;

/// Observable account lifecycle. Mapped from TDLib authorization states by the adapter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TelegramAccountStatus {
    Created,
    Initializing,
    WaitPhoneNumber,
    WaitCode,
    WaitPassword,
    WaitRegistration,
    Ready,
    Closing,
    Disconnected,
    Error,
}

impl TelegramAccountStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Initializing => "initializing",
            Self::WaitPhoneNumber => "wait_phone_number",
            Self::WaitCode => "wait_code",
            Self::WaitPassword => "wait_password",
            Self::WaitRegistration => "wait_registration",
            Self::Ready => "ready",
            Self::Closing => "closing",
            Self::Disconnected => "disconnected",
            Self::Error => "error",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "created" => Self::Created,
            "initializing" => Self::Initializing,
            "wait_phone_number" => Self::WaitPhoneNumber,
            "wait_code" => Self::WaitCode,
            "wait_password" => Self::WaitPassword,
            "wait_registration" => Self::WaitRegistration,
            "ready" => Self::Ready,
            "closing" => Self::Closing,
            "disconnected" => Self::Disconnected,
            _ => Self::Error,
        }
    }

    pub fn needs_auth_input(self) -> bool {
        matches!(
            self,
            Self::WaitPhoneNumber | Self::WaitCode | Self::WaitPassword | Self::WaitRegistration
        )
    }

    pub fn is_terminal_disconnect(self) -> bool {
        matches!(self, Self::Disconnected | Self::Closing)
    }

    /// States the user (or REST/WS clients) should observe during login / runtime.
    pub fn is_user_facing(self) -> bool {
        matches!(
            self,
            Self::WaitPhoneNumber
                | Self::WaitCode
                | Self::WaitPassword
                | Self::WaitRegistration
                | Self::Ready
                | Self::Error
                | Self::Disconnected
        )
    }
}

impl std::fmt::Display for TelegramAccountStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TelegramAccountProfile {
    pub phone_masked: Option<String>,
    pub username: Option<String>,
    pub display_name: Option<String>,
    pub telegram_user_id: Option<i64>,
}

impl Default for TelegramAccountProfile {
    fn default() -> Self {
        Self {
            phone_masked: None,
            username: None,
            display_name: None,
            telegram_user_id: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TelegramAccount {
    pub id: TelegramAccountId,
    pub status: TelegramAccountStatus,
    pub phone_masked: Option<String>,
    pub username: Option<String>,
    pub display_name: Option<String>,
    pub permissions: TelegramAccountPermissions,
    pub desired_running: bool,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl TelegramAccount {
    pub fn profile(&self) -> TelegramAccountProfile {
        TelegramAccountProfile {
            phone_masked: self.phone_masked.clone(),
            username: self.username.clone(),
            display_name: self.display_name.clone(),
            telegram_user_id: None,
        }
    }
}

/// On-disk session metadata. TDLib database files are NOT stored here.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TelegramSession {
    pub account_id: TelegramAccountId,
    pub root_dir: String,
    pub authorized: bool,
}

/// Public create response (consent is mandatory).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TelegramAccountCreated {
    pub account: TelegramAccount,
    pub consent: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TelegramAccountStatusView {
    pub account: TelegramAccount,
}
