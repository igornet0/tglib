//! Per-account tdjson client: receive thread + request/response matching.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use parking_lot::Mutex;
use serde_json::{json, Value};
use tokio::sync::{broadcast, mpsc, oneshot, watch};
use uuid::Uuid;

use tglib_client::TelegramClientUpdate;
use tglib_core::{TelegramAccountStatus, TelegramError};

use crate::config::TdlibCredentials;
use crate::ffi::TdjsonClient;
use crate::mapping::{
    extract_auth_state_from_update, json_type, map_authorization_state, parse_chat_title_update,
    parse_delete_messages_update, parse_message_content_update, parse_message_edited_update,
    parse_new_message_update, redact_json, tdlib_error_message, AuthPhase,
};

const EVENT_CAPACITY: usize = 256;

struct Pending {
    map: HashMap<String, oneshot::Sender<Value>>,
}

pub struct TdlibRuntime {
    client: Arc<TdjsonClient>,
    pending: Arc<Mutex<Pending>>,
    events: broadcast::Sender<TelegramClientUpdate>,
    status: watch::Sender<TelegramAccountStatus>,
    shutdown: Arc<AtomicBool>,
    recv_thread: Mutex<Option<JoinHandle<()>>>,
    database_directory: std::path::PathBuf,
    files_directory: std::path::PathBuf,
    credentials: TdlibCredentials,
    sent_nested_parameters: AtomicBool,
}

impl TdlibRuntime {
    pub fn spawn(
        database_directory: std::path::PathBuf,
        files_directory: std::path::PathBuf,
        credentials: TdlibCredentials,
    ) -> Result<Arc<Self>, TelegramError> {
        let _ = TdjsonClient::execute_json(
            r#"{"@type":"setLogVerbosityLevel","new_verbosity_level":1}"#,
        );
        let client = TdjsonClient::create().ok_or_else(|| {
            TelegramError::Unavailable("td_json_client_create returned null".into())
        })?;
        let client = Arc::new(client);
        let (events, _) = broadcast::channel(EVENT_CAPACITY);
        let (status, _) = watch::channel(TelegramAccountStatus::Initializing);
        let shutdown = Arc::new(AtomicBool::new(false));
        let pending = Arc::new(Mutex::new(Pending {
            map: HashMap::new(),
        }));
        let (raw_tx, raw_rx) = mpsc::unbounded_channel::<String>();

        let runtime = Arc::new(Self {
            client: client.clone(),
            pending: pending.clone(),
            events,
            status,
            shutdown: shutdown.clone(),
            recv_thread: Mutex::new(None),
            database_directory,
            files_directory,
            credentials,
            sent_nested_parameters: AtomicBool::new(false),
        });

        let recv_client = client.clone();
        let recv_shutdown = shutdown.clone();
        let handle = std::thread::Builder::new()
            .name("tdjson-recv".into())
            .spawn(move || {
                while !recv_shutdown.load(Ordering::Relaxed) {
                    if let Some(payload) = recv_client.receive(0.5) {
                        if raw_tx.send(payload).is_err() {
                            break;
                        }
                    }
                }
            })
            .map_err(|e| TelegramError::Account(e.to_string()))?;
        *runtime.recv_thread.lock() = Some(handle);

        let dispatch_runtime = runtime.clone();
        tokio::spawn(async move {
            dispatch_runtime.dispatch_loop(raw_rx).await;
        });

        Ok(runtime)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<TelegramClientUpdate> {
        self.events.subscribe()
    }

    pub fn status(&self) -> TelegramAccountStatus {
        *self.status.borrow()
    }

    async fn dispatch_loop(&self, mut raw_rx: mpsc::UnboundedReceiver<String>) {
        while let Some(payload) = raw_rx.recv().await {
            let parsed: Value = match serde_json::from_str(&payload) {
                Ok(v) => v,
                Err(_) => continue,
            };
            if let Some(extra) = extra_key(&parsed) {
                if let Some(tx) = self.pending.lock().map.remove(&extra) {
                    let _ = tx.send(parsed);
                    continue;
                }
            }
            self.handle_update(&parsed);
        }
    }

    fn handle_update(&self, update: &Value) {
        let kind = json_type(update);
        if kind == "error" {
            let message = update
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("");
            tracing::warn!(message, "telegram.tdlib.error");
            // New TDLib (1.8.4+) wants flat setTdlibParameters. Old 1.8.0 wanted nested.
            // Prefer flat; only retry nested once if the flat shape was rejected.
            let wants_nested = message.contains("tdlibParameters")
                || message.contains("parameters")
                || message.contains("database_directory")
                || message.contains("Unexpected");
            if wants_nested
                && matches!(
                    *self.status.borrow(),
                    TelegramAccountStatus::Initializing | TelegramAccountStatus::Created
                )
                && !self.sent_nested_parameters.swap(true, Ordering::Relaxed)
            {
                tracing::info!("telegram.tdlib.retry_nested_parameters");
                self.send_tdlib_parameters_nested();
            }
            return;
        }

        if let Some(state) = extract_auth_state_from_update(update) {
            let state_type = json_type(&state);
            tracing::info!(state = state_type, "telegram.tdlib.authorization_state");
            if let Some(phase) = map_authorization_state(&state) {
                match phase {
                    AuthPhase::WaitTdlibParameters => {
                        let _ = self.status.send(TelegramAccountStatus::Initializing);
                        // TDLib ≥ 1.8.4 / HEAD: flat object. Fallback to nested on error.
                        self.send_tdlib_parameters_flat();
                    }
                    AuthPhase::WaitEncryptionKey => {
                        let _ = self.status.send(TelegramAccountStatus::Initializing);
                        self.send_fire_and_forget(json!({
                            "@type": "checkDatabaseEncryptionKey",
                            "encryption_key": ""
                        }));
                    }
                    AuthPhase::UserFacing(status) => {
                        let _ = self.status.send(status);
                        let error = if status == TelegramAccountStatus::Error {
                            Some("other device confirmation is not supported".into())
                        } else {
                            None
                        };
                        tracing::info!(status = %status, "telegram.tdlib.auth_state");
                        let _ = self.events.send(TelegramClientUpdate::AuthState { status, error });
                        if status == TelegramAccountStatus::Ready {
                            let _ = self.events.send(TelegramClientUpdate::ConnectionChanged {
                                connected: true,
                            });
                        }
                        if status == TelegramAccountStatus::Disconnected {
                            let _ = self.events.send(TelegramClientUpdate::Closed);
                        }
                    }
                }
            } else {
                tracing::debug!(state = state_type, "telegram.tdlib.authorization_state_ignored");
            }
            return;
        }

        if kind == "updateConnectionState" {
            let connected = update
                .get("state")
                .and_then(|s| s.get("@type"))
                .and_then(Value::as_str)
                == Some("connectionStateReady");
            let _ = self
                .events
                .send(TelegramClientUpdate::ConnectionChanged { connected });
            return;
        }

        if let Some(msg) = parse_new_message_update(update) {
            tracing::info!("telegram.message.received");
            let _ = self.events.send(TelegramClientUpdate::MessageReceived {
                chat_id: msg.chat_id,
                message_id: msg.message_id,
                sender_id: msg.sender_id,
                text: msg.text,
                is_outgoing: msg.is_outgoing,
                timestamp: msg.timestamp,
            });
            return;
        }

        if let Some(edited) =
            parse_message_content_update(update).or_else(|| parse_message_edited_update(update))
        {
            let _ = self.events.send(TelegramClientUpdate::MessageEdited {
                chat_id: edited.chat_id,
                message_id: edited.message_id,
                text: edited.text,
                timestamp: edited.timestamp,
            });
            return;
        }

        if let Some(deleted) = parse_delete_messages_update(update) {
            let timestamp = chrono::Utc::now();
            for message_id in deleted.message_ids {
                let _ = self.events.send(TelegramClientUpdate::MessageDeleted {
                    chat_id: deleted.chat_id,
                    message_id,
                    timestamp,
                });
            }
            return;
        }

        if let Some(chat) = parse_chat_title_update(update) {
            let _ = self.events.send(TelegramClientUpdate::ChatUpdated {
                chat_id: chat.chat_id,
                title: Some(chat.title),
                timestamp: chrono::Utc::now(),
            });
        }
    }

    fn send_tdlib_parameters_nested(&self) {
        let db = self.database_directory.to_string_lossy().to_string();
        let files = self.files_directory.to_string_lossy().to_string();
        // TDLib 1.8.0 (old Homebrew bottle) uses nested `parameters`.
        let nested = json!({
            "@type": "setTdlibParameters",
            "parameters": {
                "@type": "tdlibParameters",
                "use_test_dc": self.credentials.use_test_dc,
                "database_directory": db,
                "files_directory": files,
                "use_file_database": true,
                "use_chat_info_database": true,
                "use_message_database": true,
                "use_secret_chats": false,
                "api_id": self.credentials.api_id,
                "api_hash": self.credentials.api_hash,
                "system_language_code": "en",
                "device_model": "tglib",
                "system_version": std::env::consts::OS,
                "application_version": env!("CARGO_PKG_VERSION"),
                "enable_storage_optimizer": true,
                "ignore_file_names": true
            }
        });
        self.send_fire_and_forget(nested);
    }

    fn send_tdlib_parameters_flat(&self) {
        let db = self.database_directory.to_string_lossy().to_string();
        let files = self.files_directory.to_string_lossy().to_string();
        tracing::info!("telegram.tdlib.set_parameters_flat");
        self.send_fire_and_forget(json!({
            "@type": "setTdlibParameters",
            "use_test_dc": self.credentials.use_test_dc,
            "database_directory": db,
            "files_directory": files,
            "use_file_database": true,
            "use_chat_info_database": true,
            "use_message_database": true,
            "use_secret_chats": false,
            "api_id": self.credentials.api_id,
            "api_hash": self.credentials.api_hash,
            "system_language_code": "en",
            "device_model": "tglib",
            "system_version": std::env::consts::OS,
            "application_version": env!("CARGO_PKG_VERSION")
        }));
    }

    fn send_fire_and_forget(&self, value: Value) {
        let payload = value.to_string();
        tracing::debug!(payload = %redact_json(&value), "telegram.tdlib.send");
        self.client.send_json(&payload);
    }

    pub async fn request(&self, mut value: Value) -> Result<Value, TelegramError> {
        let extra = Uuid::now_v7().to_string();
        value["@extra"] = Value::String(extra.clone());
        let (tx, rx) = oneshot::channel();
        self.pending.lock().map.insert(extra, tx);
        tracing::debug!(payload = %redact_json(&value), "telegram.tdlib.request");
        self.client.send_json(&value.to_string());
        match tokio::time::timeout(Duration::from_secs(30), rx).await {
            Ok(Ok(response)) => {
                if let Some((code, message)) = tdlib_error_message(&response) {
                    return Err(map_tdlib_error(code, message));
                }
                Ok(response)
            }
            Ok(Err(_)) => Err(TelegramError::Account("tdlib response dropped".into())),
            Err(_) => Err(TelegramError::Account("tdlib request timed out".into())),
        }
    }

    pub async fn wait_user_facing_auth(&self, timeout: Duration) -> Result<TelegramAccountStatus, TelegramError> {
        let mut rx = self.status.subscribe();
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let current = *rx.borrow();
            if is_user_facing(current) {
                return Ok(current);
            }
            tokio::select! {
                _ = tokio::time::sleep_until(deadline) => {
                    return Err(TelegramError::Account("timed out waiting for TDLib authorization state".into()));
                }
                changed = rx.changed() => {
                    if changed.is_err() {
                        return Err(TelegramError::Account("authorization watch closed".into()));
                    }
                }
            }
        }
    }

    pub fn request_stop(&self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }

    pub fn kickstart(&self) {
        // First request starts the update stream for this client instance.
        self.send_fire_and_forget(json!({ "@type": "getOption", "name": "version" }));
    }

    pub async fn close_and_join(&self) {
        self.send_fire_and_forget(json!({ "@type": "close" }));
        let mut rx = self.status.subscribe();
        let _ = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if *rx.borrow() == TelegramAccountStatus::Disconnected {
                    break;
                }
                if rx.changed().await.is_err() {
                    break;
                }
            }
        })
        .await;
        self.shutdown.store(true, Ordering::Relaxed);
        if let Some(handle) = self.recv_thread.lock().take() {
            let _ = handle.join();
        }
    }
}

impl Drop for TdlibRuntime {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        if let Some(handle) = self.recv_thread.lock().take() {
            let _ = handle.join();
        }
    }
}

fn is_user_facing(status: TelegramAccountStatus) -> bool {
    matches!(
        status,
        TelegramAccountStatus::WaitPhoneNumber
            | TelegramAccountStatus::WaitCode
            | TelegramAccountStatus::WaitPassword
            | TelegramAccountStatus::WaitRegistration
            | TelegramAccountStatus::Ready
            | TelegramAccountStatus::Error
            | TelegramAccountStatus::Disconnected
    )
}

fn extra_key(value: &Value) -> Option<String> {
    match value.get("@extra")? {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        other => Some(other.to_string()),
    }
}

pub fn map_tdlib_error(code: i64, message: String) -> TelegramError {
    let upper = message.to_ascii_uppercase();
    if upper.contains("UPDATE_APP_TO_LOGIN") {
        return TelegramError::Account(
            "UPDATE_APP_TO_LOGIN: Homebrew TDLib 1.8.0 is too old for Telegram login. \
             Build a newer TDLib and set TDLIB_LIB_DIR (see crates/tglib-tdlib/README.md)."
                .into(),
        );
    }
    match code {
        401 => TelegramError::AuthorizationRequired,
        403 => TelegramError::Restricted,
        420 | 429 => TelegramError::RateLimited,
        400 | 404 | 406 => TelegramError::InvalidRequest(message),
        _ => TelegramError::Account(message),
    }
}
