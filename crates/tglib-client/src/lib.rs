//! tglib Telegram user client.
//!
//! TDLib types must never leak through this crate.

mod format;
mod mock;
mod ops;

pub use format::{
    escape_html, markdown_to_telegram_html, prepare_telegram_text, ParseMode,
};
pub use mock::{MockClientFactory, MockTelegramClient};
pub use ops::*;

use async_trait::async_trait;
use tokio::sync::broadcast;

use tglib_core::{
    TelegramAccountId, TelegramAccountProfile, TelegramAccountStatus, TelegramChatId,
    TelegramError, TelegramMessageId,
};

/// Updates produced by a client. `account_id` is attached by EventRouter, not here.
#[derive(Clone, Debug)]
pub enum TelegramClientUpdate {
    AuthState {
        status: TelegramAccountStatus,
        error: Option<String>,
    },
    Profile {
        profile: TelegramAccountProfile,
    },
    MessageReceived {
        chat_id: TelegramChatId,
        message_id: TelegramMessageId,
        sender_id: Option<i64>,
        text: Option<String>,
        #[allow(dead_code)]
        is_outgoing: bool,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    MessageEdited {
        chat_id: TelegramChatId,
        message_id: TelegramMessageId,
        text: Option<String>,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    MessageDeleted {
        chat_id: TelegramChatId,
        message_id: TelegramMessageId,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    ChatUpdated {
        chat_id: TelegramChatId,
        title: Option<String>,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    ConnectionChanged {
        connected: bool,
    },
    Closed,
    Error {
        message: String,
    },
}

#[async_trait]
pub trait TelegramClient: Send + Sync {
    async fn start(&self) -> Result<(), TelegramError>;
    async fn shutdown(&self) -> Result<(), TelegramError>;
    async fn auth_state(&self) -> TelegramAccountStatus;

    async fn submit_phone(&self, phone: String) -> Result<(), TelegramError>;
    async fn submit_code(&self, code: String) -> Result<(), TelegramError>;
    async fn submit_password(&self, password: String) -> Result<(), TelegramError>;
    async fn register_user(&self, request: RegisterUserRequest) -> Result<(), TelegramError>;
    async fn logout(&self) -> Result<(), TelegramError>;

    async fn get_account_info(&self) -> Result<TelegramAccountProfile, TelegramError>;
    async fn list_chats(&self, request: ListChatsRequest) -> Result<ListChatsResponse, TelegramError>;
    async fn search_chats(
        &self,
        request: SearchChatsRequest,
    ) -> Result<ListChatsResponse, TelegramError>;
    async fn get_chat(&self, chat_id: TelegramChatId) -> Result<TelegramChat, TelegramError>;
    async fn get_messages(
        &self,
        request: GetMessagesRequest,
    ) -> Result<Vec<TelegramMessage>, TelegramError>;
    async fn send_message(
        &self,
        request: SendMessageRequest,
    ) -> Result<TelegramMessageId, TelegramError>;
    async fn forward_messages(&self, request: ForwardMessagesRequest) -> Result<(), TelegramError>;
    async fn edit_message(&self, request: EditMessageRequest) -> Result<(), TelegramError>;
    async fn delete_messages(&self, request: DeleteMessagesRequest) -> Result<(), TelegramError>;
    async fn send_photo(&self, request: SendPhotoRequest) -> Result<TelegramMessageId, TelegramError>;
    async fn send_document(
        &self,
        request: SendDocumentRequest,
    ) -> Result<TelegramMessageId, TelegramError>;
    async fn download_file(&self, request: DownloadFileRequest) -> Result<DownloadedFile, TelegramError>;
    async fn join_public_chat(
        &self,
        request: JoinPublicChatRequest,
    ) -> Result<TelegramChat, TelegramError>;
    async fn leave_chat(&self, chat_id: TelegramChatId) -> Result<(), TelegramError>;

    fn subscribe(&self) -> broadcast::Receiver<TelegramClientUpdate>;
}

pub trait TelegramClientFactory: Send + Sync {
    fn create(
        &self,
        account_id: TelegramAccountId,
        session_root: &std::path::Path,
    ) -> std::sync::Arc<dyn TelegramClient>;
}
