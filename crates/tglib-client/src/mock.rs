use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use dashmap::DashMap;
use parking_lot::RwLock;
use tokio::sync::broadcast;

use tglib_core::{
    mask_phone, TelegramAccountId, TelegramAccountProfile, TelegramAccountStatus, TelegramChatId,
    TelegramError, TelegramMessageId, TelegramUserId,
};

use crate::{
    DeleteMessagesRequest, DownloadFileRequest, DownloadedFile, EditMessageRequest,
    ForwardMessagesRequest, GetMessagesRequest, JoinPublicChatRequest, ListChatsRequest,
    ListChatsResponse, RegisterUserRequest, SearchChatsRequest, SendDocumentRequest,
    SendMessageRequest, SendPhotoRequest, TelegramChat, TelegramClient, TelegramClientFactory,
    TelegramClientUpdate, TelegramMessage,
};

const AUTH_MARKER: &str = ".tglib_mock_authorized.json";
const EVENT_CAPACITY: usize = 256;

#[derive(Clone, Debug)]
pub struct RecordedSend {
    pub chat_id: TelegramChatId,
    pub text: String,
    pub message_id: TelegramMessageId,
}

#[derive(Clone, Debug)]
pub struct RecordedMediaSend {
    pub chat_id: TelegramChatId,
    pub path: String,
    pub caption: Option<String>,
    pub kind: String,
    pub message_id: TelegramMessageId,
}

struct MockInner {
    status: TelegramAccountStatus,
    error: Option<String>,
    profile: TelegramAccountProfile,
    require_password: bool,
    require_registration: bool,
    chats: Vec<TelegramChat>,
    messages: HashMap<i64, Vec<TelegramMessage>>,
    sent: Vec<RecordedSend>,
    media: Vec<RecordedMediaSend>,
    forwarded: Vec<ForwardMessagesRequest>,
    edited: Vec<EditMessageRequest>,
    deleted: Vec<DeleteMessagesRequest>,
}

pub struct MockTelegramClient {
    account_id: TelegramAccountId,
    session_root: PathBuf,
    inner: RwLock<MockInner>,
    events: broadcast::Sender<TelegramClientUpdate>,
    next_message_id: AtomicI64,
}

impl MockTelegramClient {
    pub fn new(account_id: TelegramAccountId, session_root: PathBuf) -> Self {
        let (events, _) = broadcast::channel(EVENT_CAPACITY);
        Self {
            account_id,
            session_root,
            inner: RwLock::new(MockInner {
                status: TelegramAccountStatus::Created,
                error: None,
                profile: TelegramAccountProfile::default(),
                require_password: false,
                require_registration: false,
                chats: Vec::new(),
                messages: HashMap::new(),
                sent: Vec::new(),
                media: Vec::new(),
                forwarded: Vec::new(),
                edited: Vec::new(),
                deleted: Vec::new(),
            }),
            events,
            next_message_id: AtomicI64::new(1),
        }
    }

    pub fn account_id(&self) -> TelegramAccountId {
        self.account_id
    }

    pub fn set_require_password(&self, value: bool) {
        self.inner.write().require_password = value;
    }

    pub fn set_require_registration(&self, value: bool) {
        self.inner.write().require_registration = value;
    }

    pub fn recorded_sends(&self) -> Vec<RecordedSend> {
        self.inner.read().sent.clone()
    }

    pub fn recorded_media(&self) -> Vec<RecordedMediaSend> {
        self.inner.read().media.clone()
    }

    pub fn recorded_forwards(&self) -> Vec<ForwardMessagesRequest> {
        self.inner.read().forwarded.clone()
    }

    pub fn seed_chat(&self, chat: TelegramChat) {
        self.inner.write().chats.push(chat);
    }

    pub fn seed_message(&self, message: TelegramMessage) {
        self.inner
            .write()
            .messages
            .entry(message.chat_id.0)
            .or_default()
            .push(message);
    }

    pub fn emit(&self, update: TelegramClientUpdate) {
        let _ = self.events.send(update);
    }

    pub async fn inject_incoming_message(
        &self,
        chat_id: TelegramChatId,
        message_id: TelegramMessageId,
        text: impl Into<String>,
        sender_id: Option<i64>,
    ) {
        self.inject_message(chat_id, message_id, text, sender_id, false)
            .await;
    }

    /// Inject a message update (incoming or outgoing) for sandbox tests.
    pub async fn inject_message(
        &self,
        chat_id: TelegramChatId,
        message_id: TelegramMessageId,
        text: impl Into<String>,
        sender_id: Option<i64>,
        is_outgoing: bool,
    ) {
        let timestamp = Utc::now();
        let text = Some(text.into());
        self.emit(TelegramClientUpdate::MessageReceived {
            chat_id,
            message_id,
            sender_id,
            text,
            is_outgoing,
            timestamp,
        });
    }

    pub async fn force_error(&self, message: impl Into<String>) {
        let message = message.into();
        {
            let mut inner = self.inner.write();
            inner.status = TelegramAccountStatus::Error;
            inner.error = Some(message.clone());
        }
        self.emit(TelegramClientUpdate::Error {
            message: message.clone(),
        });
        self.emit(TelegramClientUpdate::AuthState {
            status: TelegramAccountStatus::Error,
            error: Some(message),
        });
    }

    fn persist_authorized(&self, profile: &TelegramAccountProfile) {
        let path = self.session_root.join(AUTH_MARKER);
        if let Ok(bytes) = serde_json::to_vec(profile) {
            let _ = std::fs::write(path, bytes);
        }
    }

    fn load_authorized(&self) -> Option<TelegramAccountProfile> {
        let path = self.session_root.join(AUTH_MARKER);
        let bytes = std::fs::read(path).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    fn emit_auth(&self, status: TelegramAccountStatus, error: Option<String>) {
        self.emit(TelegramClientUpdate::AuthState { status, error });
    }

    fn require_ready(&self) -> Result<(), TelegramError> {
        let status = self.inner.read().status;
        if status == TelegramAccountStatus::Ready {
            Ok(())
        } else if status.needs_auth_input() {
            Err(TelegramError::AuthorizationRequired)
        } else {
            Err(TelegramError::NotReady)
        }
    }
}

#[async_trait]
impl TelegramClient for MockTelegramClient {
    async fn start(&self) -> Result<(), TelegramError> {
        if let Some(profile) = self.load_authorized() {
            {
                let mut inner = self.inner.write();
                inner.status = TelegramAccountStatus::Ready;
                inner.profile = profile.clone();
                inner.error = None;
            }
            tracing::info!(account_id = %self.account_id, "telegram.account.ready");
            self.emit(TelegramClientUpdate::Profile { profile });
            self.emit_auth(TelegramAccountStatus::Ready, None);
            self.emit(TelegramClientUpdate::ConnectionChanged { connected: true });
            return Ok(());
        }

        {
            let mut inner = self.inner.write();
            inner.status = TelegramAccountStatus::WaitPhoneNumber;
            inner.error = None;
        }
        tracing::info!(account_id = %self.account_id, "telegram.auth.wait_phone");
        self.emit_auth(TelegramAccountStatus::WaitPhoneNumber, None);
        Ok(())
    }

    async fn shutdown(&self) -> Result<(), TelegramError> {
        {
            let mut inner = self.inner.write();
            inner.status = TelegramAccountStatus::Closing;
        }
        self.emit_auth(TelegramAccountStatus::Closing, None);
        {
            let mut inner = self.inner.write();
            inner.status = TelegramAccountStatus::Disconnected;
        }
        self.emit(TelegramClientUpdate::Closed);
        self.emit_auth(TelegramAccountStatus::Disconnected, None);
        Ok(())
    }

    async fn auth_state(&self) -> TelegramAccountStatus {
        self.inner.read().status
    }

    async fn submit_phone(&self, phone: String) -> Result<(), TelegramError> {
        if phone.trim().is_empty() {
            return Err(TelegramError::InvalidRequest("phone required".into()));
        }
        let masked = mask_phone(&phone);
        {
            let mut inner = self.inner.write();
            if inner.status != TelegramAccountStatus::WaitPhoneNumber
                && inner.status != TelegramAccountStatus::Created
                && inner.status != TelegramAccountStatus::Initializing
            {
                return Err(TelegramError::InvalidRequest("not waiting for phone".into()));
            }
            inner.profile.phone_masked = Some(masked);
            inner.status = TelegramAccountStatus::WaitCode;
        }
        tracing::info!(account_id = %self.account_id, "telegram.auth.wait_code");
        self.emit_auth(TelegramAccountStatus::WaitCode, None);
        Ok(())
    }

    async fn submit_code(&self, code: String) -> Result<(), TelegramError> {
        if code.trim().is_empty() {
            return Err(TelegramError::InvalidRequest("code required".into()));
        }
        let require_password;
        let require_registration;
        {
            let inner = self.inner.read();
            if inner.status != TelegramAccountStatus::WaitCode {
                return Err(TelegramError::InvalidRequest("not waiting for code".into()));
            }
            require_password = inner.require_password;
            require_registration = inner.require_registration;
        }
        if require_password {
            self.inner.write().status = TelegramAccountStatus::WaitPassword;
            tracing::info!(account_id = %self.account_id, "telegram.auth.wait_password");
            self.emit_auth(TelegramAccountStatus::WaitPassword, None);
            return Ok(());
        }
        if require_registration {
            self.inner.write().status = TelegramAccountStatus::WaitRegistration;
            tracing::info!(account_id = %self.account_id, "telegram.auth.wait_registration");
            self.emit_auth(TelegramAccountStatus::WaitRegistration, None);
            return Ok(());
        }
        self.finish_ready();
        Ok(())
    }

    async fn submit_password(&self, password: String) -> Result<(), TelegramError> {
        if password.is_empty() {
            return Err(TelegramError::InvalidRequest("password required".into()));
        }
        let require_registration;
        {
            let inner = self.inner.read();
            if inner.status != TelegramAccountStatus::WaitPassword {
                return Err(TelegramError::InvalidRequest(
                    "not waiting for password".into(),
                ));
            }
            require_registration = inner.require_registration;
        }
        if require_registration {
            self.inner.write().status = TelegramAccountStatus::WaitRegistration;
            tracing::info!(account_id = %self.account_id, "telegram.auth.wait_registration");
            self.emit_auth(TelegramAccountStatus::WaitRegistration, None);
            return Ok(());
        }
        self.finish_ready();
        Ok(())
    }

    async fn register_user(&self, request: RegisterUserRequest) -> Result<(), TelegramError> {
        let first = request.first_name.trim();
        if first.is_empty() {
            return Err(TelegramError::InvalidRequest("first_name required".into()));
        }
        {
            let inner = self.inner.read();
            if inner.status != TelegramAccountStatus::WaitRegistration {
                return Err(TelegramError::InvalidRequest(
                    "not waiting for registration".into(),
                ));
            }
        }
        {
            let mut inner = self.inner.write();
            let last = request.last_name.as_deref().unwrap_or("").trim();
            inner.profile.display_name = Some(if last.is_empty() {
                first.to_string()
            } else {
                format!("{first} {last}")
            });
        }
        self.finish_ready();
        Ok(())
    }

    async fn logout(&self) -> Result<(), TelegramError> {
        let _ = std::fs::remove_file(self.session_root.join(AUTH_MARKER));
        {
            let mut inner = self.inner.write();
            inner.status = TelegramAccountStatus::Disconnected;
            inner.profile = TelegramAccountProfile::default();
        }
        self.emit(TelegramClientUpdate::Closed);
        self.emit_auth(TelegramAccountStatus::Disconnected, None);
        Ok(())
    }

    async fn get_account_info(&self) -> Result<TelegramAccountProfile, TelegramError> {
        Ok(self.inner.read().profile.clone())
    }

    async fn list_chats(&self, request: ListChatsRequest) -> Result<ListChatsResponse, TelegramError> {
        self.require_ready()?;
        let mut chats = self.inner.read().chats.clone();
        if let Some(limit) = request.limit {
            chats.truncate(limit as usize);
        }
        Ok(ListChatsResponse { chats })
    }

    async fn search_chats(
        &self,
        request: SearchChatsRequest,
    ) -> Result<ListChatsResponse, TelegramError> {
        self.require_ready()?;
        let q = request.query.trim().trim_start_matches('@').to_ascii_lowercase();
        if q.is_empty() {
            return Err(TelegramError::InvalidRequest("search query required".into()));
        }
        let limit = request.limit.unwrap_or(20) as usize;
        let chats = self
            .inner
            .read()
            .chats
            .iter()
            .filter(|c| {
                c.title.to_ascii_lowercase().contains(&q)
                    || c.username
                        .as_ref()
                        .map(|u| u.to_ascii_lowercase().contains(&q))
                        .unwrap_or(false)
                    || c.id.0.to_string().contains(&q)
            })
            .take(limit)
            .cloned()
            .collect();
        Ok(ListChatsResponse { chats })
    }

    async fn get_chat(&self, chat_id: TelegramChatId) -> Result<TelegramChat, TelegramError> {
        self.require_ready()?;
        self.inner
            .read()
            .chats
            .iter()
            .find(|c| c.id == chat_id)
            .cloned()
            .ok_or_else(|| TelegramError::InvalidRequest("chat not found".into()))
    }

    async fn get_messages(
        &self,
        request: GetMessagesRequest,
    ) -> Result<Vec<TelegramMessage>, TelegramError> {
        self.require_ready()?;
        let mut messages = self
            .inner
            .read()
            .messages
            .get(&request.chat_id.0)
            .cloned()
            .unwrap_or_default();
        if let Some(limit) = request.limit {
            messages.truncate(limit as usize);
        }
        Ok(messages)
    }

    async fn send_message(
        &self,
        request: SendMessageRequest,
    ) -> Result<TelegramMessageId, TelegramError> {
        self.require_ready()?;
        if request.text.is_empty() {
            return Err(TelegramError::InvalidRequest("text required".into()));
        }
        let id = TelegramMessageId(self.next_message_id.fetch_add(1, Ordering::Relaxed));
        let sender_id;
        let text = request.text.clone();
        {
            let mut inner = self.inner.write();
            sender_id = inner.profile.telegram_user_id.map(TelegramUserId);
            inner.sent.push(RecordedSend {
                chat_id: request.chat_id,
                text: text.clone(),
                message_id: id,
            });
            inner
                .messages
                .entry(request.chat_id.0)
                .or_default()
                .push(TelegramMessage {
                    chat_id: request.chat_id,
                    message_id: id,
                    sender_id,
                    text: Some(text.clone()),
                    content_kind: "text".into(),
                    timestamp: Utc::now(),
                });
        }
        // Mirror TDLib: outgoing sends also produce updateNewMessage (is_outgoing=true).
        self.emit(TelegramClientUpdate::MessageReceived {
            chat_id: request.chat_id,
            message_id: id,
            sender_id: sender_id.map(|u| u.0),
            text: Some(text),
            is_outgoing: true,
            timestamp: Utc::now(),
        });
        Ok(id)
    }

    async fn forward_messages(&self, request: ForwardMessagesRequest) -> Result<(), TelegramError> {
        self.require_ready()?;
        self.inner.write().forwarded.push(request);
        Ok(())
    }

    async fn edit_message(&self, request: EditMessageRequest) -> Result<(), TelegramError> {
        self.require_ready()?;
        let chat_id = request.chat_id;
        let message_id = request.message_id;
        let text = request.text.clone();
        {
            let mut inner = self.inner.write();
            if let Some(messages) = inner.messages.get_mut(&chat_id.0) {
                if let Some(existing) = messages.iter_mut().find(|m| m.message_id == message_id) {
                    existing.text = Some(text.clone());
                }
            }
            inner.edited.push(request);
        }
        self.emit(TelegramClientUpdate::MessageEdited {
            chat_id,
            message_id,
            text: Some(text),
            timestamp: Utc::now(),
        });
        Ok(())
    }

    async fn delete_messages(&self, request: DeleteMessagesRequest) -> Result<(), TelegramError> {
        self.require_ready()?;
        let chat_id = request.chat_id;
        let ids = request.message_ids.clone();
        {
            let mut inner = self.inner.write();
            if let Some(messages) = inner.messages.get_mut(&chat_id.0) {
                messages.retain(|m| !ids.contains(&m.message_id));
            }
            inner.deleted.push(request);
        }
        let timestamp = Utc::now();
        for message_id in ids {
            self.emit(TelegramClientUpdate::MessageDeleted {
                chat_id,
                message_id,
                timestamp,
            });
        }
        Ok(())
    }

    async fn send_photo(&self, request: SendPhotoRequest) -> Result<TelegramMessageId, TelegramError> {
        self.send_media("photo", request.chat_id, request.path, request.caption)
            .await
    }

    async fn send_document(
        &self,
        request: SendDocumentRequest,
    ) -> Result<TelegramMessageId, TelegramError> {
        self.send_media("document", request.chat_id, request.path, request.caption)
            .await
    }

    async fn download_file(&self, request: DownloadFileRequest) -> Result<DownloadedFile, TelegramError> {
        self.require_ready()?;
        let dir = self.session_root.join("files");
        let _ = std::fs::create_dir_all(&dir);
        let dest = dir.join(format!("file_{}", request.file_id));
        if !dest.exists() {
            std::fs::write(&dest, b"mock").map_err(|e| TelegramError::Account(e.to_string()))?;
        }
        let size = dest.metadata().map(|m| m.len() as i64).unwrap_or(0);
        Ok(DownloadedFile {
            file_id: request.file_id,
            local_path: dest.to_string_lossy().into_owned(),
            size,
        })
    }

    async fn join_public_chat(
        &self,
        request: JoinPublicChatRequest,
    ) -> Result<TelegramChat, TelegramError> {
        self.require_ready()?;
        let username = request
            .username
            .trim()
            .trim_start_matches('@')
            .to_string();
        if username.is_empty() {
            return Err(TelegramError::InvalidRequest("username required".into()));
        }
        {
            let inner = self.inner.read();
            if let Some(existing) = inner.chats.iter().find(|c| {
                c.username
                    .as_ref()
                    .map(|u| u.eq_ignore_ascii_case(&username))
                    .unwrap_or(false)
            }) {
                return Ok(existing.clone());
            }
        }
        let chat = TelegramChat {
            id: TelegramChatId(-(self.next_message_id.fetch_add(1, Ordering::Relaxed))),
            title: username.clone(),
            chat_type: "channel".into(),
            username: Some(username),
        };
        self.inner.write().chats.push(chat.clone());
        self.emit(TelegramClientUpdate::ChatUpdated {
            chat_id: chat.id,
            title: Some(chat.title.clone()),
            timestamp: Utc::now(),
        });
        Ok(chat)
    }

    async fn leave_chat(&self, chat_id: TelegramChatId) -> Result<(), TelegramError> {
        self.require_ready()?;
        self.inner.write().chats.retain(|c| c.id != chat_id);
        Ok(())
    }

    fn subscribe(&self) -> broadcast::Receiver<TelegramClientUpdate> {
        self.events.subscribe()
    }
}

impl MockTelegramClient {
    fn finish_ready(&self) {
        let profile = {
            let mut inner = self.inner.write();
            if inner.profile.username.is_none() {
                inner.profile.username = Some(format!("user_{}", &self.account_id.to_string()[..8]));
            }
            if inner.profile.display_name.is_none() {
                inner.profile.display_name = Some("tglib User".into());
            }
            if inner.profile.telegram_user_id.is_none() {
                inner.profile.telegram_user_id = Some(1_000_000);
            }
            inner.status = TelegramAccountStatus::Ready;
            inner.error = None;
            inner.profile.clone()
        };
        self.persist_authorized(&profile);
        tracing::info!(account_id = %self.account_id, "telegram.account.ready");
        self.emit(TelegramClientUpdate::Profile {
            profile: profile.clone(),
        });
        self.emit_auth(TelegramAccountStatus::Ready, None);
        self.emit(TelegramClientUpdate::ConnectionChanged { connected: true });
    }

    async fn send_media(
        &self,
        kind: &str,
        chat_id: TelegramChatId,
        path: String,
        caption: Option<String>,
    ) -> Result<TelegramMessageId, TelegramError> {
        self.require_ready()?;
        if path.trim().is_empty() {
            return Err(TelegramError::InvalidRequest("path required".into()));
        }
        let id = TelegramMessageId(self.next_message_id.fetch_add(1, Ordering::Relaxed));
        let sender_id;
        let text = caption.clone();
        {
            let mut inner = self.inner.write();
            sender_id = inner.profile.telegram_user_id.map(TelegramUserId);
            inner.media.push(RecordedMediaSend {
                chat_id,
                path,
                caption: caption.clone(),
                kind: kind.to_string(),
                message_id: id,
            });
            inner
                .messages
                .entry(chat_id.0)
                .or_default()
                .push(TelegramMessage {
                    chat_id,
                    message_id: id,
                    sender_id,
                    text: text.clone(),
                    content_kind: kind.to_string(),
                    timestamp: Utc::now(),
                });
        }
        self.emit(TelegramClientUpdate::MessageReceived {
            chat_id,
            message_id: id,
            sender_id: sender_id.map(|u| u.0),
            text,
            is_outgoing: true,
            timestamp: Utc::now(),
        });
        Ok(id)
    }
}

#[derive(Default)]
pub struct MockClientFactory {
    clients: DashMap<TelegramAccountId, Arc<MockTelegramClient>>,
}

impl MockClientFactory {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, account_id: TelegramAccountId) -> Option<Arc<MockTelegramClient>> {
        self.clients.get(&account_id).map(|c| c.clone())
    }
}

impl TelegramClientFactory for MockClientFactory {
    fn create(
        &self,
        account_id: TelegramAccountId,
        session_root: &Path,
    ) -> Arc<dyn TelegramClient> {
        let client = Arc::new(MockTelegramClient::new(account_id, session_root.to_path_buf()));
        self.clients.insert(account_id, client.clone());
        client
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DownloadFileRequest, JoinPublicChatRequest, RegisterUserRequest, SendPhotoRequest};

    fn temp_root() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tglib-mock-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    async fn ready_client() -> MockTelegramClient {
        let client = MockTelegramClient::new(TelegramAccountId::new(), temp_root());
        client.start().await.unwrap();
        client.submit_phone("+35840111222".into()).await.unwrap();
        client.submit_code("12345".into()).await.unwrap();
        assert_eq!(client.auth_state().await, TelegramAccountStatus::Ready);
        client
    }

    #[tokio::test]
    async fn register_user_after_code() {
        let client = MockTelegramClient::new(TelegramAccountId::new(), temp_root());
        client.set_require_registration(true);
        client.start().await.unwrap();
        client.submit_phone("+35840111222".into()).await.unwrap();
        client.submit_code("12345".into()).await.unwrap();
        assert_eq!(
            client.auth_state().await,
            TelegramAccountStatus::WaitRegistration
        );
        client
            .register_user(RegisterUserRequest {
                first_name: "Ada".into(),
                last_name: Some("Lovelace".into()),
            })
            .await
            .unwrap();
        assert_eq!(client.auth_state().await, TelegramAccountStatus::Ready);
        let profile = client.get_account_info().await.unwrap();
        assert_eq!(profile.display_name.as_deref(), Some("Ada Lovelace"));
    }

    #[tokio::test]
    async fn send_photo_and_download_file() {
        let client = ready_client().await;
        let chat = TelegramChatId(42);
        client.seed_chat(TelegramChat {
            id: chat,
            title: "lab".into(),
            chat_type: "private".into(),
            username: None,
        });
        let id = client
            .send_photo(SendPhotoRequest {
                chat_id: chat,
                path: "/tmp/shot.png".into(),
                caption: Some("hello".into()),
                parse_mode: crate::ParseMode::Plain,
            })
            .await
            .unwrap();
        let media = client.recorded_media();
        assert_eq!(media.len(), 1);
        assert_eq!(media[0].kind, "photo");
        assert_eq!(media[0].message_id, id);
        let file = client
            .download_file(DownloadFileRequest {
                file_id: 7,
                priority: None,
            })
            .await
            .unwrap();
        assert_eq!(file.file_id, 7);
        assert!(std::path::Path::new(&file.local_path).exists());
    }

    #[tokio::test]
    async fn join_and_leave_public_chat() {
        let client = ready_client().await;
        let chat = client
            .join_public_chat(JoinPublicChatRequest {
                username: "@news".into(),
            })
            .await
            .unwrap();
        assert_eq!(chat.username.as_deref(), Some("news"));
        client.leave_chat(chat.id).await.unwrap();
        assert!(client.get_chat(chat.id).await.is_err());
    }

    #[tokio::test]
    async fn edit_and_delete_emit_updates() {
        let client = ready_client().await;
        let chat = TelegramChatId(1);
        let mut rx = client.subscribe();
        let id = client
            .send_message(SendMessageRequest {
                chat_id: chat,
                text: "one".into(),
                parse_mode: crate::ParseMode::Plain,
            })
            .await
            .unwrap();
        client
            .edit_message(EditMessageRequest {
                chat_id: chat,
                message_id: id,
                text: "two".into(),
                parse_mode: crate::ParseMode::Plain,
            })
            .await
            .unwrap();
        client
            .delete_messages(DeleteMessagesRequest {
                chat_id: chat,
                message_ids: vec![id],
            })
            .await
            .unwrap();

        let mut saw_edit = false;
        let mut saw_delete = false;
        while let Ok(update) = rx.try_recv() {
            match update {
                TelegramClientUpdate::MessageEdited { text, .. } => {
                    saw_edit = text.as_deref() == Some("two");
                }
                TelegramClientUpdate::MessageDeleted { message_id, .. } => {
                    saw_delete = message_id == id;
                }
                _ => {}
            }
        }
        assert!(saw_edit);
        assert!(saw_delete);
    }
}
