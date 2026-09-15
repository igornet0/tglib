//! Map TDLib JSON objects onto tglib domain types.
//! Never expose TDLib `@type` names outside this crate's adapter boundary.

use chrono::{TimeZone, Utc};
use serde_json::Value;

use tglib_client::TelegramChat;
use tglib_core::{
    mask_phone, TelegramAccountProfile, TelegramAccountStatus, TelegramChatId, TelegramMessageId,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthPhase {
    WaitTdlibParameters,
    WaitEncryptionKey,
    UserFacing(TelegramAccountStatus),
}

pub fn json_type(value: &Value) -> &str {
    value
        .get("@type")
        .and_then(Value::as_str)
        .unwrap_or("")
}

pub fn map_authorization_state(state: &Value) -> Option<AuthPhase> {
    match json_type(state) {
        "authorizationStateWaitTdlibParameters" => Some(AuthPhase::WaitTdlibParameters),
        "authorizationStateWaitEncryptionKey" => Some(AuthPhase::WaitEncryptionKey),
        "authorizationStateWaitPhoneNumber" => {
            Some(AuthPhase::UserFacing(TelegramAccountStatus::WaitPhoneNumber))
        }
        "authorizationStateWaitCode" => {
            Some(AuthPhase::UserFacing(TelegramAccountStatus::WaitCode))
        }
        "authorizationStateWaitPassword" => {
            Some(AuthPhase::UserFacing(TelegramAccountStatus::WaitPassword))
        }
        "authorizationStateWaitRegistration" => {
            Some(AuthPhase::UserFacing(TelegramAccountStatus::WaitRegistration))
        }
        "authorizationStateReady" => Some(AuthPhase::UserFacing(TelegramAccountStatus::Ready)),
        "authorizationStateLoggingOut" | "authorizationStateClosing" => {
            Some(AuthPhase::UserFacing(TelegramAccountStatus::Closing))
        }
        "authorizationStateClosed" => {
            Some(AuthPhase::UserFacing(TelegramAccountStatus::Disconnected))
        }
        "authorizationStateWaitOtherDeviceConfirmation" => {
            Some(AuthPhase::UserFacing(TelegramAccountStatus::Error))
        }
        _ => None,
    }
}

pub fn extract_auth_state_from_update(update: &Value) -> Option<Value> {
    if json_type(update) != "updateAuthorizationState" {
        return None;
    }
    update.get("authorization_state").cloned()
}

pub fn profile_from_user(user: &Value) -> TelegramAccountProfile {
    let first = user.get("first_name").and_then(Value::as_str).unwrap_or("");
    let last = user.get("last_name").and_then(Value::as_str).unwrap_or("");
    let display = format!("{first} {last}").trim().to_string();
    let username = user
        .get("username")
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            user.get("usernames")
                .and_then(|u| u.get("active_usernames"))
                .and_then(Value::as_array)
                .and_then(|a| a.first())
                .and_then(Value::as_str)
                .map(str::to_string)
        });
    let phone_masked = user
        .get("phone_number")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(mask_phone);
    TelegramAccountProfile {
        phone_masked,
        username,
        display_name: if display.is_empty() {
            None
        } else {
            Some(display)
        },
        telegram_user_id: user.get("id").and_then(Value::as_i64),
    }
}

pub fn chat_from_tdlib(chat: &Value) -> Option<TelegramChat> {
    let id = chat.get("id").and_then(Value::as_i64)?;
    let title = chat
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let type_obj = chat.get("type");
    let type_name = type_obj
        .and_then(|t| t.get("@type"))
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let chat_type = match type_name {
        "chatTypePrivate" => "private".to_string(),
        "chatTypeBasicGroup" => "group".to_string(),
        "chatTypeSupergroup" => {
            if type_obj
                .and_then(|t| t.get("is_channel"))
                .and_then(Value::as_bool)
                == Some(true)
            {
                "channel".to_string()
            } else {
                "supergroup".to_string()
            }
        }
        "chatTypeSecret" => "secret".to_string(),
        other => other.to_string(),
    };
    let username = chat
        .get("username")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    Some(TelegramChat {
        id: TelegramChatId(id),
        title: if title.is_empty() {
            username
                .clone()
                .unwrap_or_else(|| format!("chat {id}"))
        } else {
            title
        },
        chat_type,
        username,
    })
}

pub struct IncomingMessage {
    pub chat_id: TelegramChatId,
    pub message_id: TelegramMessageId,
    pub sender_id: Option<i64>,
    pub text: Option<String>,
    pub content_kind: String,
    pub is_outgoing: bool,
    pub timestamp: chrono::DateTime<Utc>,
}

pub fn message_from_tdlib(message: &Value) -> Option<IncomingMessage> {
    let chat_id = message.get("chat_id").and_then(Value::as_i64)?;
    let message_id = message.get("id").and_then(Value::as_i64)?;
    let sender_id = message
        .get("sender_id")
        .and_then(|s| s.get("user_id"))
        .and_then(Value::as_i64)
        .or_else(|| {
            message
                .get("sender_user_id")
                .and_then(Value::as_i64)
        });
    let content = message.get("content");
    let text = content.and_then(extract_text);
    let content_kind = content
        .map(content_kind)
        .unwrap_or_else(|| "other".into());
    let is_outgoing = message
        .get("is_outgoing")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let ts = message
        .get("date")
        .and_then(Value::as_i64)
        .and_then(|sec| Utc.timestamp_opt(sec, 0).single())
        .unwrap_or_else(Utc::now);
    Some(IncomingMessage {
        chat_id: TelegramChatId(chat_id),
        message_id: TelegramMessageId(message_id),
        sender_id,
        text,
        content_kind,
        is_outgoing,
        timestamp: ts,
    })
}

pub fn content_kind(content: &Value) -> String {
    match json_type(content) {
        "messageText" => "text".into(),
        "messagePhoto" => "photo".into(),
        "messageDocument" => "document".into(),
        "messageVoiceNote" | "messageAudio" => "voice".into(),
        _ => "other".into(),
    }
}

pub fn extract_text(content: &Value) -> Option<String> {
    match json_type(content) {
        "messageText" => content
            .get("text")
            .and_then(|t| t.get("text"))
            .and_then(Value::as_str)
            .map(str::to_string),
        _ => content
            .get("caption")
            .and_then(|t| t.get("text"))
            .and_then(Value::as_str)
            .map(str::to_string),
    }
}

pub fn parse_new_message_update(update: &Value) -> Option<IncomingMessage> {
    if json_type(update) != "updateNewMessage" {
        return None;
    }
    message_from_tdlib(update.get("message")?)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditedMessage {
    pub chat_id: TelegramChatId,
    pub message_id: TelegramMessageId,
    pub text: Option<String>,
    pub timestamp: chrono::DateTime<Utc>,
}

pub fn parse_message_edited_update(update: &Value) -> Option<EditedMessage> {
    if json_type(update) != "updateMessageEdited" {
        return None;
    }
    let chat_id = update.get("chat_id").and_then(Value::as_i64)?;
    let message_id = update.get("message_id").and_then(Value::as_i64)?;
    let timestamp = update
        .get("edit_date")
        .and_then(Value::as_i64)
        .and_then(|sec| Utc.timestamp_opt(sec, 0).single())
        .unwrap_or_else(Utc::now);
    let text = update
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(extract_text);
    Some(EditedMessage {
        chat_id: TelegramChatId(chat_id),
        message_id: TelegramMessageId(message_id),
        text,
        timestamp,
    })
}

pub fn parse_message_content_update(update: &Value) -> Option<EditedMessage> {
    if json_type(update) != "updateMessageContent" {
        return None;
    }
    let chat_id = update.get("chat_id").and_then(Value::as_i64)?;
    let message_id = update.get("message_id").and_then(Value::as_i64)?;
    let text = update.get("new_content").and_then(extract_text);
    Some(EditedMessage {
        chat_id: TelegramChatId(chat_id),
        message_id: TelegramMessageId(message_id),
        text,
        timestamp: Utc::now(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeletedMessages {
    pub chat_id: TelegramChatId,
    pub message_ids: Vec<TelegramMessageId>,
}

pub fn parse_delete_messages_update(update: &Value) -> Option<DeletedMessages> {
    if json_type(update) != "updateDeleteMessages" {
        return None;
    }
    let chat_id = update.get("chat_id").and_then(Value::as_i64)?;
    let message_ids = update
        .get("message_ids")
        .and_then(Value::as_array)?
        .iter()
        .filter_map(Value::as_i64)
        .map(TelegramMessageId)
        .collect::<Vec<_>>();
    if message_ids.is_empty() {
        return None;
    }
    Some(DeletedMessages {
        chat_id: TelegramChatId(chat_id),
        message_ids,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatTitleUpdate {
    pub chat_id: TelegramChatId,
    pub title: String,
}

pub fn parse_chat_title_update(update: &Value) -> Option<ChatTitleUpdate> {
    if json_type(update) != "updateChatTitle" {
        return None;
    }
    let chat_id = update.get("chat_id").and_then(Value::as_i64)?;
    let title = update
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    Some(ChatTitleUpdate {
        chat_id: TelegramChatId(chat_id),
        title,
    })
}

/// Redact fields that must never appear in logs.
pub fn redact_json(value: &Value) -> Value {
    const SECRET_KEYS: &[&str] = &[
        "phone_number",
        "password",
        "code",
        "api_hash",
        "encryption_key",
        "phone_number_hash",
    ];
    match value {
        Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (k, v) in map {
                if SECRET_KEYS.contains(&k.as_str()) {
                    out.insert(k.clone(), Value::String("[redacted]".into()));
                } else {
                    out.insert(k.clone(), redact_json(v));
                }
            }
            Value::Object(out)
        }
        Value::Array(arr) => Value::Array(arr.iter().map(redact_json).collect()),
        other => other.clone(),
    }
}

pub fn tdlib_error_message(value: &Value) -> Option<(i64, String)> {
    if json_type(value) != "error" {
        return None;
    }
    let code = value.get("code").and_then(Value::as_i64).unwrap_or(0);
    let message = value
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("tdlib error")
        .to_string();
    Some((code, message))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn maps_wait_phone_and_ready() {
        assert_eq!(
            map_authorization_state(&json!({"@type": "authorizationStateWaitPhoneNumber"})),
            Some(AuthPhase::UserFacing(TelegramAccountStatus::WaitPhoneNumber))
        );
        assert_eq!(
            map_authorization_state(&json!({"@type": "authorizationStateWaitTdlibParameters"})),
            Some(AuthPhase::WaitTdlibParameters)
        );
        assert_eq!(
            map_authorization_state(&json!({"@type": "authorizationStateWaitRegistration"})),
            Some(AuthPhase::UserFacing(TelegramAccountStatus::WaitRegistration))
        );
        assert_eq!(
            map_authorization_state(&json!({"@type": "authorizationStateReady"})),
            Some(AuthPhase::UserFacing(TelegramAccountStatus::Ready))
        );
    }

    #[test]
    fn maps_new_text_message() {
        let update = json!({
            "@type": "updateNewMessage",
            "message": {
                "id": 77,
                "chat_id": 42,
                "date": 1_700_000_000,
                "sender_id": {"@type": "messageSenderUser", "user_id": 9},
                "content": {
                    "@type": "messageText",
                    "text": {"@type": "formattedText", "text": "ping"}
                }
            }
        });
        let msg = parse_new_message_update(&update).expect("mapped");
        assert_eq!(msg.chat_id.0, 42);
        assert_eq!(msg.message_id.0, 77);
        assert_eq!(msg.sender_id, Some(9));
        assert_eq!(msg.text.as_deref(), Some("ping"));
        assert_eq!(msg.content_kind, "text");
        assert!(!msg.is_outgoing);
    }

    #[test]
    fn maps_photo_caption() {
        let update = json!({
            "@type": "updateNewMessage",
            "message": {
                "id": 2,
                "chat_id": 5,
                "date": 1_700_000_000,
                "content": {
                    "@type": "messagePhoto",
                    "caption": {"@type": "formattedText", "text": "shot"}
                }
            }
        });
        let msg = parse_new_message_update(&update).expect("mapped");
        assert_eq!(msg.content_kind, "photo");
        assert_eq!(msg.text.as_deref(), Some("shot"));
    }

    #[test]
    fn maps_message_edited_and_content() {
        let edited = parse_message_edited_update(&json!({
            "@type": "updateMessageEdited",
            "chat_id": 10,
            "message_id": 11,
            "edit_date": 1_700_000_100
        }))
        .expect("edited");
        assert_eq!(edited.chat_id.0, 10);
        assert_eq!(edited.message_id.0, 11);
        assert!(edited.text.is_none());

        let content = parse_message_content_update(&json!({
            "@type": "updateMessageContent",
            "chat_id": 10,
            "message_id": 11,
            "new_content": {
                "@type": "messageText",
                "text": {"@type": "formattedText", "text": "updated"}
            }
        }))
        .expect("content");
        assert_eq!(content.text.as_deref(), Some("updated"));
    }

    #[test]
    fn maps_delete_and_chat_title() {
        let deleted = parse_delete_messages_update(&json!({
            "@type": "updateDeleteMessages",
            "chat_id": 3,
            "message_ids": [1, 2]
        }))
        .expect("deleted");
        assert_eq!(deleted.chat_id.0, 3);
        assert_eq!(deleted.message_ids.len(), 2);

        let title = parse_chat_title_update(&json!({
            "@type": "updateChatTitle",
            "chat_id": 8,
            "title": "Renamed"
        }))
        .expect("title");
        assert_eq!(title.title, "Renamed");
    }

    #[test]
    fn maps_outgoing_flag() {
        let update = json!({
            "@type": "updateNewMessage",
            "message": {
                "id": 1,
                "chat_id": 2,
                "date": 1_700_000_000,
                "is_outgoing": true,
                "sender_id": {"@type": "messageSenderUser", "user_id": 9},
                "content": {
                    "@type": "messageText",
                    "text": {"@type": "formattedText", "text": "reply"}
                }
            }
        });
        let msg = parse_new_message_update(&update).expect("mapped");
        assert!(msg.is_outgoing);
    }

    #[test]
    fn redacts_auth_secrets() {
        let raw = json!({
            "@type": "checkAuthenticationPassword",
            "password": "super-secret",
            "api_hash": "abcd",
            "nested": { "phone_number": "+358111" }
        });
        let redacted = redact_json(&raw);
        let s = redacted.to_string();
        assert!(!s.contains("super-secret"));
        assert!(!s.contains("abcd"));
        assert!(!s.contains("+358111"));
        assert!(s.contains("[redacted]"));
    }

    #[test]
    fn masks_phone_on_profile() {
        let user = json!({
            "id": 1,
            "first_name": "Ada",
            "last_name": "Lovelace",
            "username": "ada",
            "phone_number": "35840111222"
        });
        let profile = profile_from_user(&user);
        assert_eq!(profile.display_name.as_deref(), Some("Ada Lovelace"));
        assert_eq!(profile.username.as_deref(), Some("ada"));
        let masked = profile.phone_masked.expect("masked");
        assert!(masked.contains("***"));
        assert!(!masked.contains("40111222"));
    }
}
