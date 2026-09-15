use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::ids::{TelegramAccountId, TelegramChatId, TelegramMessageId, TelegramUserId};
use crate::TelegramAccountStatus;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TelegramMessageReceived {
    pub account_id: TelegramAccountId,
    pub chat_id: TelegramChatId,
    pub message_id: TelegramMessageId,
    pub sender_id: Option<TelegramUserId>,
    pub text: Option<String>,
    /// TDLib `is_outgoing` — true when the connected account sent the message.
    #[serde(default)]
    pub is_outgoing: bool,
    pub timestamp: DateTime<Utc>,
}

impl TelegramMessageReceived {
    pub fn event_key(&self) -> String {
        format!(
            "{}|{}|{}|message_received",
            self.account_id, self.chat_id, self.message_id
        )
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TelegramMessageEdited {
    pub account_id: TelegramAccountId,
    pub chat_id: TelegramChatId,
    pub message_id: TelegramMessageId,
    pub text: Option<String>,
    pub timestamp: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TelegramMessageDeleted {
    pub account_id: TelegramAccountId,
    pub chat_id: TelegramChatId,
    pub message_id: TelegramMessageId,
    pub timestamp: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TelegramChatUpdated {
    pub account_id: TelegramAccountId,
    pub chat_id: TelegramChatId,
    pub title: Option<String>,
    pub timestamp: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TelegramUserUpdated {
    pub account_id: TelegramAccountId,
    pub user_id: TelegramUserId,
    pub username: Option<String>,
    pub timestamp: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TelegramAuthorizationChanged {
    pub account_id: TelegramAccountId,
    pub status: TelegramAccountStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub timestamp: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TelegramConnectionChanged {
    pub account_id: TelegramAccountId,
    pub connected: bool,
    pub timestamp: DateTime<Utc>,
}

/// Domain event. Every variant includes `account_id`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum TelegramEvent {
    MessageReceived(TelegramMessageReceived),
    MessageEdited(TelegramMessageEdited),
    MessageDeleted(TelegramMessageDeleted),
    ChatUpdated(TelegramChatUpdated),
    UserUpdated(TelegramUserUpdated),
    AuthorizationChanged(TelegramAuthorizationChanged),
    ConnectionChanged(TelegramConnectionChanged),
}

impl TelegramEvent {
    pub fn account_id(&self) -> TelegramAccountId {
        match self {
            Self::MessageReceived(p) => p.account_id,
            Self::MessageEdited(p) => p.account_id,
            Self::MessageDeleted(p) => p.account_id,
            Self::ChatUpdated(p) => p.account_id,
            Self::UserUpdated(p) => p.account_id,
            Self::AuthorizationChanged(p) => p.account_id,
            Self::ConnectionChanged(p) => p.account_id,
        }
    }

    pub fn event_type(&self) -> &'static str {
        match self {
            Self::MessageReceived(_) => "message_received",
            Self::MessageEdited(_) => "message_edited",
            Self::MessageDeleted(_) => "message_deleted",
            Self::ChatUpdated(_) => "chat_updated",
            Self::UserUpdated(_) => "user_updated",
            Self::AuthorizationChanged(_) => "authorization_changed",
            Self::ConnectionChanged(_) => "connection_changed",
        }
    }

    pub fn event_key(&self) -> Option<String> {
        match self {
            Self::MessageReceived(p) => Some(p.event_key()),
            Self::MessageEdited(p) => Some(format!(
                "{}|{}|{}|message_edited",
                p.account_id, p.chat_id, p.message_id
            )),
            Self::MessageDeleted(p) => Some(format!(
                "{}|{}|{}|message_deleted",
                p.account_id, p.chat_id, p.message_id
            )),
            _ => None,
        }
    }
}

/// Typed auth-flow events for the frontend (mapped from TDLib authorization states).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TelegramAuthEvent {
    PhoneNumberRequired { account_id: TelegramAccountId },
    CodeRequired { account_id: TelegramAccountId },
    PasswordRequired { account_id: TelegramAccountId },
    Ready { account_id: TelegramAccountId },
    Closed { account_id: TelegramAccountId },
    Error { account_id: TelegramAccountId, message: String },
}

impl TelegramAuthEvent {
    pub fn from_status(account_id: TelegramAccountId, status: TelegramAccountStatus, error: Option<String>) -> Self {
        match status {
            TelegramAccountStatus::WaitPhoneNumber => Self::PhoneNumberRequired { account_id },
            TelegramAccountStatus::WaitCode => Self::CodeRequired { account_id },
            TelegramAccountStatus::WaitPassword => Self::PasswordRequired { account_id },
            TelegramAccountStatus::Ready => Self::Ready { account_id },
            TelegramAccountStatus::Disconnected | TelegramAccountStatus::Closing => {
                Self::Closed { account_id }
            }
            TelegramAccountStatus::Error => Self::Error {
                account_id,
                message: error.unwrap_or_else(|| "account error".into()),
            },
            _ => Self::PhoneNumberRequired { account_id },
        }
    }
}
