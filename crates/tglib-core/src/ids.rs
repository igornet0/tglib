use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Stable internal account identity. Never use a phone number as the primary key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TelegramAccountId(pub Uuid);

impl TelegramAccountId {
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl Default for TelegramAccountId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for TelegramAccountId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<Uuid> for TelegramAccountId {
    fn from(value: Uuid) -> Self {
        Self(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TelegramChatId(pub i64);

impl std::fmt::Display for TelegramChatId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TelegramMessageId(pub i64);

impl std::fmt::Display for TelegramMessageId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TelegramUserId(pub i64);

impl std::fmt::Display for TelegramUserId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
