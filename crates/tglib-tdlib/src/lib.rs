//! TDLib adapter for [`tglib_client::TelegramClient`].
//!
//! Default / CI builds compile the stub (no `tdjson`).
//! `cargo test -p tglib-tdlib --features native` links Homebrew/system tdjson.

mod config;
mod mapping;

#[cfg(feature = "native")]
mod adapter;
#[cfg(feature = "native")]
mod ffi;
#[cfg(feature = "native")]
mod runtime;

#[cfg(not(feature = "native"))]
mod stub;

#[cfg(feature = "native")]
pub use adapter::{TdlibAdapter, TdlibClientFactory};

#[cfg(not(feature = "native"))]
pub use stub::{TdlibAdapter, TdlibClientFactory};

pub use config::{
    TelegramClientConfig, TelegramConfigInput, TelegramConfigProvider, TelegramConfigResolve,
    TelegramConfigSource, TdlibCredentials, SECRET_API_HASH, SECRET_API_ID,
};
pub use mapping::{
    map_authorization_state, parse_chat_title_update, parse_delete_messages_update,
    parse_message_content_update, parse_message_edited_update, parse_new_message_update, AuthPhase,
};

#[cfg(all(test, not(feature = "native")))]
mod stub_tests {
    use super::*;
    use std::path::PathBuf;

    use tglib_client::TelegramClient;
    use tglib_core::TelegramAccountId;

    #[tokio::test]
    async fn stub_start_returns_unavailable() {
        let adapter = TdlibAdapter::new(TelegramAccountId::new(), PathBuf::from("/tmp/tglib-tdlib-stub"));
        let err = adapter.start().await.expect_err("stub must not start");
        assert!(err.to_string().contains("tdjson"));
    }
}

#[cfg(all(test, feature = "native"))]
mod native_smoke {
    use std::time::Duration;

    use tglib_core::TelegramAccountStatus;

    use crate::config::TdlibCredentials;
    use crate::runtime::TdlibRuntime;

    #[tokio::test]
    async fn tdjson_leaves_initializing() {
        let dir = std::env::temp_dir().join(format!("tglib-tdjson-{}", uuid::Uuid::now_v7()));
        let db = dir.join("database");
        let files = dir.join("files");
        std::fs::create_dir_all(&db).expect("database dir");
        std::fs::create_dir_all(&files).expect("files dir");

        let runtime = TdlibRuntime::spawn(
            db,
            files,
            TdlibCredentials {
                api_id: 1,
                api_hash: "0123456789abcdef0123456789abcdef".into(),
                use_test_dc: true,
            },
        )
        .expect("td_json_client_create must succeed when tdjson is linked");
        runtime.kickstart();

        let status = runtime
            .wait_user_facing_auth(Duration::from_secs(30))
            .await
            .expect("native TDLib must emit a user-facing authorization state");
        assert!(
            matches!(
                status,
                TelegramAccountStatus::WaitPhoneNumber
                    | TelegramAccountStatus::WaitCode
                    | TelegramAccountStatus::WaitPassword
                    | TelegramAccountStatus::WaitRegistration
                    | TelegramAccountStatus::Ready
                    | TelegramAccountStatus::Error
            ),
            "unexpected native auth status: {status}"
        );

        runtime.close_and_join().await;
        let _ = std::fs::remove_dir_all(dir);
    }
}
