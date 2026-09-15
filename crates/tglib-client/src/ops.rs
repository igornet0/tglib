use serde::{Deserialize, Serialize};

use tglib_core::{TelegramChatId, TelegramMessageId, TelegramUserId};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ListChatsRequest {
    pub limit: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ListChatsResponse {
    pub chats: Vec<TelegramChat>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TelegramChat {
    pub id: TelegramChatId,
    pub title: String,
    pub chat_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SearchChatsRequest {
    pub query: String,
    pub limit: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GetMessagesRequest {
    pub chat_id: TelegramChatId,
    pub limit: Option<u32>,
}

fn default_content_kind() -> String {
    "text".into()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TelegramMessage {
    pub chat_id: TelegramChatId,
    pub message_id: TelegramMessageId,
    pub sender_id: Option<TelegramUserId>,
    /// Message text, or caption for media.
    pub text: Option<String>,
    /// `text`, `photo`, `document`, `voice`, or `other`.
    #[serde(default = "default_content_kind")]
    pub content_kind: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SendMessageRequest {
    pub chat_id: TelegramChatId,
    pub text: String,
    #[serde(default)]
    pub parse_mode: crate::ParseMode,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ForwardMessagesRequest {
    pub from_chat_id: TelegramChatId,
    pub to_chat_id: TelegramChatId,
    pub message_ids: Vec<TelegramMessageId>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EditMessageRequest {
    pub chat_id: TelegramChatId,
    pub message_id: TelegramMessageId,
    pub text: String,
    #[serde(default)]
    pub parse_mode: crate::ParseMode,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeleteMessagesRequest {
    pub chat_id: TelegramChatId,
    pub message_ids: Vec<TelegramMessageId>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RegisterUserRequest {
    pub first_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_name: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SendPhotoRequest {
    pub chat_id: TelegramChatId,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caption: Option<String>,
    #[serde(default)]
    pub parse_mode: crate::ParseMode,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SendDocumentRequest {
    pub chat_id: TelegramChatId,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caption: Option<String>,
    #[serde(default)]
    pub parse_mode: crate::ParseMode,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DownloadFileRequest {
    pub file_id: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<i32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DownloadedFile {
    pub file_id: i32,
    pub local_path: String,
    pub size: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JoinPublicChatRequest {
    pub username: String,
}
