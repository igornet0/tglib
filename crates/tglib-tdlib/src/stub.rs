//! Stub used when the `native` feature is off so workspace/CI builds skip tdjson.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::Mutex;
use tokio::sync::broadcast;

use tglib_client::{
    DeleteMessagesRequest, DownloadFileRequest, DownloadedFile, EditMessageRequest,
    ForwardMessagesRequest, GetMessagesRequest, JoinPublicChatRequest, ListChatsRequest,
    ListChatsResponse, RegisterUserRequest, SearchChatsRequest, SendDocumentRequest,
    SendMessageRequest, SendPhotoRequest, TelegramChat, TelegramClient, TelegramClientFactory,
    TelegramClientUpdate, TelegramMessage,
};
use tglib_core::{
    TelegramAccountId, TelegramAccountProfile, TelegramAccountStatus, TelegramChatId,
    TelegramError, TelegramMessageId,
};

const EVENT_CAPACITY: usize = 32;

fn unavailable() -> TelegramError {
    TelegramError::Unavailable(
        "TDLib native library (tdjson) is not linked. Rebuild with `--features native` after installing TDLib."
            .into(),
    )
}

pub struct TdlibAdapter {
    account_id: TelegramAccountId,
    session_root: PathBuf,
    status: Mutex<TelegramAccountStatus>,
    events: broadcast::Sender<TelegramClientUpdate>,
}

impl TdlibAdapter {
    pub fn new(account_id: TelegramAccountId, session_root: PathBuf) -> Self {
        let (events, _) = broadcast::channel(EVENT_CAPACITY);
        let _ = std::fs::create_dir_all(session_root.join("tdlib"));
        let _ = std::fs::create_dir_all(session_root.join("database"));
        let _ = std::fs::create_dir_all(session_root.join("files"));
        Self {
            account_id,
            session_root,
            status: Mutex::new(TelegramAccountStatus::Error),
            events,
        }
    }

    pub fn session_root(&self) -> &Path {
        &self.session_root
    }

    pub fn account_id(&self) -> TelegramAccountId {
        self.account_id
    }
}

#[async_trait]
impl TelegramClient for TdlibAdapter {
    async fn start(&self) -> Result<(), TelegramError> {
        *self.status.lock() = TelegramAccountStatus::Error;
        let err = unavailable();
        let _ = self.events.send(TelegramClientUpdate::AuthState {
            status: TelegramAccountStatus::Error,
            error: Some(err.to_string()),
        });
        Err(err)
    }

    async fn shutdown(&self) -> Result<(), TelegramError> {
        *self.status.lock() = TelegramAccountStatus::Disconnected;
        let _ = self.events.send(TelegramClientUpdate::Closed);
        Ok(())
    }

    async fn auth_state(&self) -> TelegramAccountStatus {
        *self.status.lock()
    }

    async fn submit_phone(&self, _phone: String) -> Result<(), TelegramError> {
        Err(unavailable())
    }

    async fn submit_code(&self, _code: String) -> Result<(), TelegramError> {
        Err(unavailable())
    }

    async fn submit_password(&self, _password: String) -> Result<(), TelegramError> {
        Err(unavailable())
    }

    async fn register_user(&self, _request: RegisterUserRequest) -> Result<(), TelegramError> {
        Err(unavailable())
    }

    async fn logout(&self) -> Result<(), TelegramError> {
        self.shutdown().await
    }

    async fn get_account_info(&self) -> Result<TelegramAccountProfile, TelegramError> {
        Err(unavailable())
    }

    async fn list_chats(
        &self,
        _request: ListChatsRequest,
    ) -> Result<ListChatsResponse, TelegramError> {
        Err(unavailable())
    }

    async fn search_chats(
        &self,
        _request: SearchChatsRequest,
    ) -> Result<ListChatsResponse, TelegramError> {
        Err(unavailable())
    }

    async fn get_chat(&self, _chat_id: TelegramChatId) -> Result<TelegramChat, TelegramError> {
        Err(unavailable())
    }

    async fn get_messages(
        &self,
        _request: GetMessagesRequest,
    ) -> Result<Vec<TelegramMessage>, TelegramError> {
        Err(unavailable())
    }

    async fn send_message(
        &self,
        _request: SendMessageRequest,
    ) -> Result<TelegramMessageId, TelegramError> {
        Err(unavailable())
    }

    async fn forward_messages(&self, _request: ForwardMessagesRequest) -> Result<(), TelegramError> {
        Err(unavailable())
    }

    async fn edit_message(&self, _request: EditMessageRequest) -> Result<(), TelegramError> {
        Err(unavailable())
    }

    async fn delete_messages(&self, _request: DeleteMessagesRequest) -> Result<(), TelegramError> {
        Err(unavailable())
    }

    async fn send_photo(&self, _request: SendPhotoRequest) -> Result<TelegramMessageId, TelegramError> {
        Err(unavailable())
    }

    async fn send_document(
        &self,
        _request: SendDocumentRequest,
    ) -> Result<TelegramMessageId, TelegramError> {
        Err(unavailable())
    }

    async fn download_file(
        &self,
        _request: DownloadFileRequest,
    ) -> Result<DownloadedFile, TelegramError> {
        Err(unavailable())
    }

    async fn join_public_chat(
        &self,
        _request: JoinPublicChatRequest,
    ) -> Result<TelegramChat, TelegramError> {
        Err(unavailable())
    }

    async fn leave_chat(&self, _chat_id: TelegramChatId) -> Result<(), TelegramError> {
        Err(unavailable())
    }

    fn subscribe(&self) -> broadcast::Receiver<TelegramClientUpdate> {
        self.events.subscribe()
    }
}

#[derive(Default)]
pub struct TdlibClientFactory;

impl TdlibClientFactory {
    pub fn new() -> Self {
        Self
    }

    pub fn from_env() -> Result<Self, TelegramError> {
        Ok(Self)
    }
}

impl TelegramClientFactory for TdlibClientFactory {
    fn create(
        &self,
        account_id: TelegramAccountId,
        session_root: &Path,
    ) -> Arc<dyn TelegramClient> {
        Arc::new(TdlibAdapter::new(account_id, session_root.to_path_buf()))
    }
}
