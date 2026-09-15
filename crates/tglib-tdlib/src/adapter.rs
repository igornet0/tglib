use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use parking_lot::Mutex;
use serde_json::{json, Value};
use tokio::sync::broadcast;

use tglib_client::{
    prepare_telegram_text, DeleteMessagesRequest, DownloadFileRequest, DownloadedFile,
    EditMessageRequest, ForwardMessagesRequest, GetMessagesRequest, JoinPublicChatRequest,
    ListChatsRequest, ListChatsResponse, ParseMode, RegisterUserRequest, SearchChatsRequest,
    SendDocumentRequest, SendMessageRequest, SendPhotoRequest, TelegramChat, TelegramClient,
    TelegramClientFactory, TelegramClientUpdate, TelegramMessage,
};
use tglib_core::{
    TelegramAccountId, TelegramAccountProfile, TelegramAccountStatus, TelegramChatId,
    TelegramError, TelegramMessageId, TelegramUserId,
};

use crate::config::TdlibCredentials;
use crate::mapping::{chat_from_tdlib, message_from_tdlib, profile_from_user};
use crate::runtime::TdlibRuntime;

pub struct TdlibAdapter {
    account_id: TelegramAccountId,
    session_root: PathBuf,
    credentials: TdlibCredentials,
    runtime: Mutex<Option<Arc<TdlibRuntime>>>,
    events: broadcast::Sender<TelegramClientUpdate>,
}

impl TdlibAdapter {
    pub fn new(
        account_id: TelegramAccountId,
        session_root: PathBuf,
        credentials: TdlibCredentials,
    ) -> Self {
        let _ = std::fs::create_dir_all(session_root.join("database"));
        let _ = std::fs::create_dir_all(session_root.join("files"));
        let _ = std::fs::create_dir_all(session_root.join("tdlib"));
        let (events, _) = broadcast::channel(256);
        tracing::info!(account_id = %account_id, "telegram.tdlib.adapter_native");
        Self {
            account_id,
            session_root,
            credentials,
            runtime: Mutex::new(None),
            events,
        }
    }

    fn runtime(&self) -> Result<Arc<TdlibRuntime>, TelegramError> {
        self.runtime
            .lock()
            .clone()
            .ok_or(TelegramError::NotReady)
    }

    async fn resolve_formatted_text(
        runtime: &TdlibRuntime,
        text: &str,
        parse_mode: ParseMode,
    ) -> Value {
        let (body, use_html) = prepare_telegram_text(text, parse_mode);
        if use_html.is_none() {
            return json!({ "@type": "formattedText", "text": body, "entities": [] });
        }
        match runtime
            .request(json!({
                "@type": "parseTextEntities",
                "text": body,
                "parse_mode": { "@type": "textParseModeHTML" }
            }))
            .await
        {
            Ok(formatted)
                if formatted.get("@type").and_then(Value::as_str) == Some("formattedText") =>
            {
                formatted
            }
            Ok(_) | Err(_) => {
                tracing::warn!("telegram.format.parse_failed — sending plain text");
                json!({
                    "@type": "formattedText",
                    "text": text,
                    "entities": []
                })
            }
        }
    }

    async fn ensure_started(&self) -> Result<Arc<TdlibRuntime>, TelegramError> {
        if let Some(rt) = self.runtime.lock().clone() {
            return Ok(rt);
        }
        self.start().await?;
        self.runtime()
    }

    async fn get_chat_enriched(&self, chat_id: TelegramChatId) -> Result<TelegramChat, TelegramError> {
        let runtime = self.runtime()?;
        let chat = runtime
            .request(json!({ "@type": "getChat", "chat_id": chat_id.0 }))
            .await?;
        let mapped = chat_from_tdlib(&chat)
            .ok_or_else(|| TelegramError::InvalidRequest("chat not found".into()))?;
        Ok(self.enrich_username(mapped).await)
    }

    async fn enrich_username(&self, mut chat: TelegramChat) -> TelegramChat {
        if chat.username.is_some() {
            return chat;
        }
        let Ok(runtime) = self.runtime() else {
            return chat;
        };
        let Ok(raw) = runtime
            .request(json!({ "@type": "getChat", "chat_id": chat.id.0 }))
            .await
        else {
            return chat;
        };
        let type_obj = raw.get("type");
        let type_name = type_obj
            .and_then(|t| t.get("@type"))
            .and_then(Value::as_str)
            .unwrap_or("");
        if type_name == "chatTypeSupergroup" {
            if let Some(sg_id) = type_obj
                .and_then(|t| t.get("supergroup_id"))
                .and_then(Value::as_i64)
            {
                if let Ok(sg) = runtime
                    .request(json!({ "@type": "getSupergroup", "supergroup_id": sg_id }))
                    .await
                {
                    chat.username = sg
                        .get("username")
                        .and_then(Value::as_str)
                        .filter(|s| !s.is_empty())
                        .map(str::to_string)
                        .or_else(|| {
                            sg.get("usernames")
                                .and_then(|u| u.get("active_usernames"))
                                .and_then(Value::as_array)
                                .and_then(|a| a.first())
                                .and_then(Value::as_str)
                                .map(str::to_string)
                        });
                }
            }
        }
        chat
    }

    async fn caption_formatted(
        runtime: &TdlibRuntime,
        caption: Option<&str>,
        parse_mode: ParseMode,
    ) -> Value {
        match caption {
            Some(text) if !text.is_empty() => {
                Self::resolve_formatted_text(runtime, text, parse_mode).await
            }
            _ => json!({ "@type": "formattedText", "text": "", "entities": [] }),
        }
    }

    fn message_id(result: &Value) -> Result<TelegramMessageId, TelegramError> {
        result
            .get("id")
            .and_then(Value::as_i64)
            .map(TelegramMessageId)
            .ok_or_else(|| TelegramError::Account("sendMessage returned no id".into()))
    }
}

#[async_trait]
impl TelegramClient for TdlibAdapter {
    async fn start(&self) -> Result<(), TelegramError> {
        tracing::info!(account_id = %self.account_id, "telegram.tdlib.start");
        if self.credentials.api_id == 0 || self.credentials.api_hash.is_empty() {
            return Err(TelegramError::InvalidRequest(
                "Telegram application credentials are not configured".into(),
            ));
        }
        if self.runtime.lock().is_some() {
            return Ok(());
        }

        let database_directory = self.session_root.join("database");
        let files_directory = self.session_root.join("files");
        let _ = std::fs::create_dir_all(&database_directory);
        let _ = std::fs::create_dir_all(&files_directory);

        let runtime = TdlibRuntime::spawn(
            database_directory,
            files_directory,
            TdlibCredentials {
                api_id: self.credentials.api_id,
                api_hash: self.credentials.api_hash.clone(),
                use_test_dc: self.credentials.use_test_dc,
            },
        )?;
        {
            let mut rt = runtime.subscribe();
            let local = self.events.clone();
            tokio::spawn(async move {
                loop {
                    match rt.recv().await {
                        Ok(update) => {
                            let _ = local.send(update);
                        }
                        Err(broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(broadcast::error::RecvError::Closed) => break,
                    }
                }
            });
        }
        *self.runtime.lock() = Some(runtime.clone());
        runtime.kickstart();
        let status = runtime
            .wait_user_facing_auth(Duration::from_secs(45))
            .await?;
        if status == TelegramAccountStatus::Ready {
            if let Ok(me) = runtime.request(json!({ "@type": "getMe" })).await {
                let profile = profile_from_user(&me);
                let _ = self.events.send(TelegramClientUpdate::Profile { profile });
            }
        }
        Ok(())
    }

    async fn shutdown(&self) -> Result<(), TelegramError> {
        let runtime = self.runtime.lock().take();
        if let Some(runtime) = runtime {
            runtime.close_and_join().await;
        }
        let _ = self.events.send(TelegramClientUpdate::Closed);
        Ok(())
    }

    async fn auth_state(&self) -> TelegramAccountStatus {
        self.runtime
            .lock()
            .as_ref()
            .map(|rt| rt.status())
            .unwrap_or(TelegramAccountStatus::Created)
    }

    async fn submit_phone(&self, phone: String) -> Result<(), TelegramError> {
        let phone = phone.trim().to_string();
        if phone.is_empty() {
            return Err(TelegramError::InvalidRequest("phone required".into()));
        }
        let runtime = self.ensure_started().await?;
        // TDLib 1.8+ expects settings; omitting them can fail on some builds.
        runtime
            .request(json!({
                "@type": "setAuthenticationPhoneNumber",
                "phone_number": phone,
                "settings": {
                    "@type": "phoneNumberAuthenticationSettings",
                    "allow_flash_call": false,
                    "allow_missed_call": false,
                    "is_current_phone_number": false,
                    "allow_sms_retriever_api": false
                }
            }))
            .await?;
        Ok(())
    }

    async fn submit_code(&self, code: String) -> Result<(), TelegramError> {
        let runtime = self.runtime()?;
        runtime
            .request(json!({
                "@type": "checkAuthenticationCode",
                "code": code
            }))
            .await?;
        Ok(())
    }

    async fn submit_password(&self, password: String) -> Result<(), TelegramError> {
        let runtime = self.runtime()?;
        runtime
            .request(json!({
                "@type": "checkAuthenticationPassword",
                "password": password
            }))
            .await?;
        Ok(())
    }

    async fn register_user(&self, request: RegisterUserRequest) -> Result<(), TelegramError> {
        let first_name = request.first_name.trim().to_string();
        if first_name.is_empty() {
            return Err(TelegramError::InvalidRequest("first_name required".into()));
        }
        let runtime = self.runtime()?;
        runtime
            .request(json!({
                "@type": "registerUser",
                "first_name": first_name,
                "last_name": request.last_name.unwrap_or_default()
            }))
            .await?;
        Ok(())
    }

    async fn logout(&self) -> Result<(), TelegramError> {
        let runtime = self.runtime.lock().clone();
        if let Some(runtime) = runtime {
            let _ = runtime.request(json!({ "@type": "logOut" })).await;
            runtime.close_and_join().await;
        }
        *self.runtime.lock() = None;
        Ok(())
    }

    async fn get_account_info(&self) -> Result<TelegramAccountProfile, TelegramError> {
        let runtime = self.runtime()?;
        let me = runtime.request(json!({ "@type": "getMe" })).await?;
        Ok(profile_from_user(&me))
    }

    async fn list_chats(
        &self,
        request: ListChatsRequest,
    ) -> Result<ListChatsResponse, TelegramError> {
        let runtime = self.runtime()?;
        let limit = request.limit.unwrap_or(50) as i64;
        // Newer TDLib needs chats loaded into memory before getChats returns them.
        let _ = runtime
            .request(json!({
                "@type": "loadChats",
                "chat_list": { "@type": "chatListMain" },
                "limit": limit
            }))
            .await;
        let response = match runtime
            .request(json!({
                "@type": "getChats",
                "chat_list": { "@type": "chatListMain" },
                "limit": limit
            }))
            .await
        {
            Ok(v) => v,
            Err(_) => {
                runtime
                    .request(json!({
                        "@type": "getChats",
                        "offset_order": "9223372036854775807",
                        "offset_chat_id": 0,
                        "limit": limit
                    }))
                    .await?
            }
        };
        let mut chats = Vec::new();
        if let Some(ids) = response.get("chat_ids").and_then(Value::as_array) {
            for id in ids.iter().filter_map(Value::as_i64) {
                if let Ok(chat) = self.get_chat_enriched(TelegramChatId(id)).await {
                    chats.push(chat);
                }
            }
        }
        Ok(ListChatsResponse { chats })
    }

    async fn search_chats(
        &self,
        request: SearchChatsRequest,
    ) -> Result<ListChatsResponse, TelegramError> {
        let query = request.query.trim().trim_start_matches('@').to_string();
        if query.is_empty() {
            return Err(TelegramError::InvalidRequest("search query required".into()));
        }
        let runtime = self.runtime()?;
        let limit = request.limit.unwrap_or(20) as i64;
        let mut chats = Vec::new();
        let mut seen = std::collections::HashSet::new();

        // Exact public username / channel (@name).
        if !query.contains(' ') {
            if let Ok(chat) = runtime
                .request(json!({
                    "@type": "searchPublicChat",
                    "username": query
                }))
                .await
            {
                if let Some(mapped) = chat_from_tdlib(&chat) {
                    if seen.insert(mapped.id.0) {
                        chats.push(self.enrich_username(mapped).await);
                    }
                }
            }
        }

        // Local dialogs.
        if let Ok(local) = runtime
            .request(json!({
                "@type": "searchChats",
                "query": query,
                "limit": limit
            }))
            .await
        {
            if let Some(ids) = local.get("chat_ids").and_then(Value::as_array) {
                for id in ids.iter().filter_map(Value::as_i64) {
                    if !seen.insert(id) {
                        continue;
                    }
                    if let Ok(chat) = self.get_chat_enriched(TelegramChatId(id)).await {
                        chats.push(chat);
                    }
                }
            }
        }

        // Public global search (channels / users).
        if let Ok(public) = runtime
            .request(json!({
                "@type": "searchPublicChats",
                "query": query
            }))
            .await
        {
            if let Some(ids) = public.get("chat_ids").and_then(Value::as_array) {
                for id in ids.iter().filter_map(Value::as_i64).take(limit as usize) {
                    if !seen.insert(id) {
                        continue;
                    }
                    if let Ok(chat) = self.get_chat_enriched(TelegramChatId(id)).await {
                        chats.push(chat);
                    }
                }
            }
        }

        if chats.len() > limit as usize {
            chats.truncate(limit as usize);
        }
        Ok(ListChatsResponse { chats })
    }

    async fn get_chat(&self, chat_id: TelegramChatId) -> Result<TelegramChat, TelegramError> {
        self.get_chat_enriched(chat_id).await
    }

    async fn get_messages(
        &self,
        request: GetMessagesRequest,
    ) -> Result<Vec<TelegramMessage>, TelegramError> {
        let runtime = self.runtime()?;
        let limit = request.limit.unwrap_or(50) as i64;
        let history = runtime
            .request(json!({
                "@type": "getChatHistory",
                "chat_id": request.chat_id.0,
                "from_message_id": 0,
                "offset": 0,
                "limit": limit,
                "only_local": false
            }))
            .await?;
        let Some(messages) = history.get("messages").and_then(Value::as_array) else {
            return Ok(Vec::new());
        };
        Ok(messages
            .iter()
            .filter_map(message_from_tdlib)
            .map(|m| TelegramMessage {
                chat_id: m.chat_id,
                message_id: m.message_id,
                sender_id: m.sender_id.map(TelegramUserId),
                text: m.text,
                content_kind: m.content_kind,
                timestamp: m.timestamp,
            })
            .collect())
    }

    async fn send_message(
        &self,
        request: SendMessageRequest,
    ) -> Result<TelegramMessageId, TelegramError> {
        let runtime = self.runtime()?;
        let formatted =
            Self::resolve_formatted_text(&runtime, &request.text, request.parse_mode).await;
        let result = runtime
            .request(json!({
                "@type": "sendMessage",
                "chat_id": request.chat_id.0,
                "input_message_content": {
                    "@type": "inputMessageText",
                    "text": formatted
                }
            }))
            .await?;
        Self::message_id(&result)
    }

    async fn forward_messages(&self, request: ForwardMessagesRequest) -> Result<(), TelegramError> {
        let runtime = self.runtime()?;
        runtime
            .request(json!({
                "@type": "forwardMessages",
                "chat_id": request.to_chat_id.0,
                "from_chat_id": request.from_chat_id.0,
                "message_ids": request.message_ids.iter().map(|id| id.0).collect::<Vec<_>>()
            }))
            .await?;
        Ok(())
    }

    async fn edit_message(&self, request: EditMessageRequest) -> Result<(), TelegramError> {
        let runtime = self.runtime()?;
        let formatted =
            Self::resolve_formatted_text(&runtime, &request.text, request.parse_mode).await;
        runtime
            .request(json!({
                "@type": "editMessageText",
                "chat_id": request.chat_id.0,
                "message_id": request.message_id.0,
                "input_message_content": {
                    "@type": "inputMessageText",
                    "text": formatted
                }
            }))
            .await?;
        Ok(())
    }

    async fn delete_messages(&self, request: DeleteMessagesRequest) -> Result<(), TelegramError> {
        let runtime = self.runtime()?;
        runtime
            .request(json!({
                "@type": "deleteMessages",
                "chat_id": request.chat_id.0,
                "message_ids": request.message_ids.iter().map(|id| id.0).collect::<Vec<_>>(),
                "revoke": true
            }))
            .await?;
        Ok(())
    }

    async fn send_photo(&self, request: SendPhotoRequest) -> Result<TelegramMessageId, TelegramError> {
        if request.path.trim().is_empty() {
            return Err(TelegramError::InvalidRequest("path required".into()));
        }
        let runtime = self.runtime()?;
        let caption = Self::caption_formatted(&runtime, request.caption.as_deref(), request.parse_mode).await;
        let result = runtime
            .request(json!({
                "@type": "sendMessage",
                "chat_id": request.chat_id.0,
                "input_message_content": {
                    "@type": "inputMessagePhoto",
                    "photo": { "@type": "inputFileLocal", "path": request.path },
                    "caption": caption
                }
            }))
            .await?;
        Self::message_id(&result)
    }

    async fn send_document(
        &self,
        request: SendDocumentRequest,
    ) -> Result<TelegramMessageId, TelegramError> {
        if request.path.trim().is_empty() {
            return Err(TelegramError::InvalidRequest("path required".into()));
        }
        let runtime = self.runtime()?;
        let caption = Self::caption_formatted(&runtime, request.caption.as_deref(), request.parse_mode).await;
        let result = runtime
            .request(json!({
                "@type": "sendMessage",
                "chat_id": request.chat_id.0,
                "input_message_content": {
                    "@type": "inputMessageDocument",
                    "document": { "@type": "inputFileLocal", "path": request.path },
                    "caption": caption
                }
            }))
            .await?;
        Self::message_id(&result)
    }

    async fn download_file(&self, request: DownloadFileRequest) -> Result<DownloadedFile, TelegramError> {
        let runtime = self.runtime()?;
        let priority = request.priority.unwrap_or(32);
        let result = runtime
            .request(json!({
                "@type": "downloadFile",
                "file_id": request.file_id,
                "priority": priority,
                "offset": 0,
                "limit": 0,
                "synchronous": true
            }))
            .await?;
        let local = result.get("local").ok_or_else(|| {
            TelegramError::Account("downloadFile returned no local file".into())
        })?;
        let path = local
            .get("path")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| TelegramError::Account("downloadFile has no local path".into()))?;
        let size = local
            .get("downloaded_size")
            .and_then(Value::as_i64)
            .or_else(|| result.get("size").and_then(Value::as_i64))
            .unwrap_or(0);
        Ok(DownloadedFile {
            file_id: result
                .get("id")
                .and_then(Value::as_i64)
                .unwrap_or(request.file_id as i64) as i32,
            local_path: path.to_string(),
            size,
        })
    }

    async fn join_public_chat(
        &self,
        request: JoinPublicChatRequest,
    ) -> Result<TelegramChat, TelegramError> {
        let username = request.username.trim().trim_start_matches('@').to_string();
        if username.is_empty() {
            return Err(TelegramError::InvalidRequest("username required".into()));
        }
        let runtime = self.runtime()?;
        let chat = runtime
            .request(json!({
                "@type": "searchPublicChat",
                "username": username
            }))
            .await?;
        let mapped = chat_from_tdlib(&chat)
            .ok_or_else(|| TelegramError::InvalidRequest("public chat not found".into()))?;
        let _ = runtime
            .request(json!({
                "@type": "joinChat",
                "chat_id": mapped.id.0
            }))
            .await;
        self.get_chat_enriched(mapped.id).await
    }

    async fn leave_chat(&self, chat_id: TelegramChatId) -> Result<(), TelegramError> {
        let runtime = self.runtime()?;
        runtime
            .request(json!({
                "@type": "leaveChat",
                "chat_id": chat_id.0
            }))
            .await?;
        Ok(())
    }

    fn subscribe(&self) -> broadcast::Receiver<TelegramClientUpdate> {
        self.events.subscribe()
    }
}

impl Drop for TdlibAdapter {
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.lock().take() {
            runtime.request_stop();
        }
    }
}

#[derive(Clone)]
pub struct TdlibClientFactory {
    credentials: TdlibCredentials,
}

impl TdlibClientFactory {
    pub fn new() -> Self {
        Self {
            credentials: TdlibCredentials {
                api_id: 0,
                api_hash: String::new(),
                use_test_dc: false,
            },
        }
    }

    pub fn from_config(config: crate::config::TelegramClientConfig) -> Self {
        Self {
            credentials: config.to_credentials(),
        }
    }
}

impl Default for TdlibClientFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl TelegramClientFactory for TdlibClientFactory {
    fn create(
        &self,
        account_id: TelegramAccountId,
        session_root: &Path,
    ) -> Arc<dyn TelegramClient> {
        Arc::new(TdlibAdapter::new(
            account_id,
            session_root.to_path_buf(),
            TdlibCredentials {
                api_id: self.credentials.api_id,
                api_hash: self.credentials.api_hash.clone(),
                use_test_dc: self.credentials.use_test_dc,
            },
        ))
    }
}
