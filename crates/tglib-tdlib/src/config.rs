//! Application credentials for TDLib (`api_id` / `api_hash`).
//!
//! These identify **your Telegram application**, not a user session.
//! User sessions live under a data directory you choose (e.g. `{data}/accounts/<id>/`).
//!
//! Resolution, lowest → highest priority:
//! 1. Compile-time embed (`TGLIB_EMBEDDED_TELEGRAM_API_*`)
//! 2. `telegram.toml` (bootstrap only; hash should not stay in plaintext)
//! 3. Encrypted secret store (injected by the host app)
//! 4. macOS Keychain
//! 5. Process env (`TELEGRAM_API_ID` / `TELEGRAM_API_HASH`) — developer override
//!
//! Never log `api_hash`. Do not copy credentials from Telegram Desktop.

use std::path::{Path, PathBuf};

use serde::Deserialize;

pub const SECRET_API_ID: &str = "telegram.app.api_id";
pub const SECRET_API_HASH: &str = "telegram.app.api_hash";
#[cfg(target_os = "macos")]
const KEYCHAIN_SERVICE: &str = "dev.tglib.telegram";
#[cfg(target_os = "macos")]
const KEYCHAIN_ACCOUNT: &str = "application";

#[derive(Clone)]
pub struct TdlibCredentials {
    pub api_id: i32,
    pub api_hash: String,
    pub use_test_dc: bool,
}

impl std::fmt::Debug for TdlibCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TdlibCredentials")
            .field("api_id", &self.api_id)
            .field("api_hash", &"[redacted]")
            .field("use_test_dc", &self.use_test_dc)
            .finish()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TelegramConfigSource {
    Embedded,
    File,
    SecretStore,
    Keychain,
    EnvOverride,
}

impl std::fmt::Display for TelegramConfigSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Embedded => "embedded",
            Self::File => "file",
            Self::SecretStore => "secret_store",
            Self::Keychain => "keychain",
            Self::EnvOverride => "env",
        })
    }
}

#[derive(Clone)]
pub struct TelegramClientConfig {
    pub api_id: i32,
    pub api_hash: String,
    pub use_test_dc: bool,
    pub source: TelegramConfigSource,
}

impl std::fmt::Debug for TelegramClientConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TelegramClientConfig")
            .field("api_id", &self.api_id)
            .field("api_hash", &"[redacted]")
            .field("use_test_dc", &self.use_test_dc)
            .field("source", &self.source)
            .finish()
    }
}

impl TelegramClientConfig {
    pub fn is_complete(&self) -> bool {
        self.api_id != 0 && !self.api_hash.is_empty()
    }

    pub fn to_credentials(&self) -> TdlibCredentials {
        TdlibCredentials {
            api_id: self.api_id,
            api_hash: self.api_hash.clone(),
            use_test_dc: self.use_test_dc,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct TelegramConfigInput {
    pub data_dir: PathBuf,
    pub secret_api_id: Option<i32>,
    pub secret_api_hash: Option<String>,
}

pub struct TelegramConfigResolve {
    pub config: TelegramClientConfig,
    /// Hash came from plaintext toml — copy it into Keychain / SecretStore.
    pub persist_to_secure_storage: bool,
}

#[derive(Default, Clone)]
struct Layer {
    api_id: Option<i32>,
    api_hash: Option<String>,
    use_test_dc: Option<bool>,
    source: Option<TelegramConfigSource>,
}

impl Layer {
    #[cfg(test)]
    fn complete(&self) -> bool {
        self.api_id.unwrap_or(0) != 0 && self.api_hash.as_ref().is_some_and(|s| !s.is_empty())
    }
}

#[derive(Deserialize)]
struct TelegramTomlFile {
    telegram: Option<TelegramTomlSection>,
}

#[derive(Deserialize, Default)]
struct TelegramTomlSection {
    api_id: Option<i32>,
    api_hash: Option<String>,
    use_test_dc: Option<bool>,
}

#[derive(Deserialize)]
struct KeychainBlob {
    api_id: Option<i32>,
    api_hash: Option<String>,
    use_test_dc: Option<bool>,
}

pub struct TelegramConfigProvider;

impl TelegramConfigProvider {
    pub fn resolve(input: TelegramConfigInput) -> TelegramConfigResolve {
        let file_path = discover_config_path(&input.data_dir);
        let file_layer = file_path.as_ref().and_then(|p| read_file_layer(p));
        let file_had_hash = file_layer
            .as_ref()
            .and_then(|l| l.api_hash.as_ref())
            .is_some_and(|s| !s.is_empty());

        let mut acc = Layer::default();
        overlay(&mut acc, embedded_layer());
        if let Some(layer) = file_layer {
            overlay(&mut acc, layer);
        }
        overlay(&mut acc, secret_layer(&input));
        overlay(&mut acc, keychain_layer());
        overlay(&mut acc, env_layer());

        let persist_to_secure_storage =
            file_had_hash && acc.source == Some(TelegramConfigSource::File);

        TelegramConfigResolve {
            config: TelegramClientConfig {
                api_id: acc.api_id.unwrap_or(0),
                api_hash: acc.api_hash.unwrap_or_default(),
                use_test_dc: acc.use_test_dc.unwrap_or(false),
                source: acc.source.unwrap_or(TelegramConfigSource::File),
            },
            persist_to_secure_storage,
        }
    }

    pub fn store_in_keychain(config: &TelegramClientConfig) -> Result<(), String> {
        store_keychain_blob(config)
    }
}

fn overlay(base: &mut Layer, next: Layer) {
    let had_id = next.api_id.filter(|id| *id != 0);
    let had_hash = next.api_hash.as_ref().is_some_and(|s| !s.is_empty());
    let complete_pair = had_id.is_some() && had_hash;
    if had_id.is_some() {
        base.api_id = had_id;
    }
    if had_hash {
        base.api_hash = next.api_hash;
    }
    if let Some(dc) = next.use_test_dc {
        base.use_test_dc = Some(dc);
    }
    if complete_pair || had_hash {
        base.source = next.source;
    }
}

fn discover_config_path(data_dir: &Path) -> Option<PathBuf> {
    if let Ok(explicit) = std::env::var("TGLIB_TELEGRAM_CONFIG") {
        let p = PathBuf::from(explicit);
        if p.is_file() {
            return Some(p);
        }
    }
    let candidates = [
        data_dir.join("telegram.toml"),
        PathBuf::from("config/telegram.toml"),
        PathBuf::from("telegram.toml"),
    ];
    candidates.into_iter().find(|p| p.is_file())
}

fn read_file_layer(path: &Path) -> Option<Layer> {
    let raw = std::fs::read_to_string(path).ok()?;
    let parsed: TelegramTomlFile = toml::from_str(&raw).ok()?;
    let section = parsed.telegram?;
    Some(Layer {
        api_id: section.api_id.filter(|id| *id != 0),
        api_hash: section.api_hash.filter(|s| !s.is_empty()),
        use_test_dc: section.use_test_dc,
        source: Some(TelegramConfigSource::File),
    })
}

fn secret_layer(input: &TelegramConfigInput) -> Layer {
    Layer {
        api_id: input.secret_api_id.filter(|id| *id != 0),
        api_hash: input
            .secret_api_hash
            .clone()
            .filter(|s| !s.is_empty()),
        use_test_dc: None,
        source: Some(TelegramConfigSource::SecretStore),
    }
}

fn env_layer() -> Layer {
    let api_id = std::env::var("TELEGRAM_API_ID")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|id| *id != 0);
    let api_hash = std::env::var("TELEGRAM_API_HASH")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let use_test_dc = std::env::var("TELEGRAM_USE_TEST_DC").ok().map(|v| {
        v == "1" || v.eq_ignore_ascii_case("true")
    });
    Layer {
        api_id,
        api_hash,
        use_test_dc,
        source: Some(TelegramConfigSource::EnvOverride),
    }
}

fn embedded_layer() -> Layer {
    let api_id = option_env!("TGLIB_EMBEDDED_TELEGRAM_API_ID")
        .and_then(|s| s.parse().ok())
        .filter(|id| *id != 0);
    let api_hash = option_env!("TGLIB_EMBEDDED_TELEGRAM_API_HASH")
        .map(str::to_string)
        .filter(|s| !s.is_empty());
    let use_test_dc = option_env!("TGLIB_EMBEDDED_TELEGRAM_USE_TEST_DC").map(|v| {
        v == "1" || v.eq_ignore_ascii_case("true")
    });
    Layer {
        api_id,
        api_hash,
        use_test_dc,
        source: Some(TelegramConfigSource::Embedded),
    }
}

fn keychain_layer() -> Layer {
    match load_keychain_blob() {
        Some(blob) => Layer {
            api_id: blob.api_id.filter(|id| *id != 0),
            api_hash: blob.api_hash.filter(|s| !s.is_empty()),
            use_test_dc: blob.use_test_dc,
            source: Some(TelegramConfigSource::Keychain),
        },
        None => Layer::default(),
    }
}

#[cfg(target_os = "macos")]
fn load_keychain_blob() -> Option<KeychainBlob> {
    let entry = keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT).ok()?;
    let payload = entry.get_password().ok()?;
    serde_json::from_str(&payload).ok()
}

#[cfg(not(target_os = "macos"))]
fn load_keychain_blob() -> Option<KeychainBlob> {
    None
}

#[cfg(target_os = "macos")]
fn store_keychain_blob(config: &TelegramClientConfig) -> Result<(), String> {
    let entry = keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT)
        .map_err(|e| e.to_string())?;
    let payload = serde_json::json!({
        "api_id": config.api_id,
        "api_hash": config.api_hash,
        "use_test_dc": config.use_test_dc,
    });
    entry
        .set_password(&payload.to_string())
        .map_err(|e| e.to_string())
}

#[cfg(not(target_os = "macos"))]
fn store_keychain_blob(_config: &TelegramClientConfig) -> Result<(), String> {
    Err("keychain is only available on macOS".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolve_from_layers(layers: Vec<Layer>) -> (Layer, bool) {
        let file_had_hash = layers.iter().any(|l| {
            l.source == Some(TelegramConfigSource::File)
                && l.api_hash.as_ref().is_some_and(|s| !s.is_empty())
        });
        let mut acc = Layer::default();
        for layer in layers {
            overlay(&mut acc, layer);
        }
        let persist = file_had_hash && acc.source == Some(TelegramConfigSource::File);
        (acc, persist)
    }

    fn layer(source: TelegramConfigSource, api_id: i32, api_hash: &str) -> Layer {
        Layer {
            api_id: (api_id != 0).then_some(api_id),
            api_hash: (!api_hash.is_empty()).then(|| api_hash.to_string()),
            use_test_dc: None,
            source: Some(source),
        }
    }

    #[test]
    fn env_override_wins_over_file() {
        let (acc, persist) = resolve_from_layers(vec![
            layer(TelegramConfigSource::File, 11, "file-hash"),
            layer(TelegramConfigSource::EnvOverride, 22, "env-hash"),
        ]);
        assert_eq!(acc.api_id, Some(22));
        assert_eq!(acc.api_hash.as_deref(), Some("env-hash"));
        assert_eq!(acc.source, Some(TelegramConfigSource::EnvOverride));
        assert!(!persist);
    }

    #[test]
    fn secret_hash_combines_with_file_id() {
        let (acc, persist) = resolve_from_layers(vec![
            layer(TelegramConfigSource::File, 11, ""),
            layer(TelegramConfigSource::SecretStore, 0, "secret-hash"),
        ]);
        assert_eq!(acc.api_id, Some(11));
        assert_eq!(acc.api_hash.as_deref(), Some("secret-hash"));
        assert_eq!(acc.source, Some(TelegramConfigSource::SecretStore));
        assert!(!persist);
        assert!(acc.complete());
    }

    #[test]
    fn plaintext_file_is_marked_for_secure_persist() {
        let (acc, persist) = resolve_from_layers(vec![layer(
            TelegramConfigSource::File,
            99,
            "plaintext-hash",
        )]);
        assert_eq!(acc.source, Some(TelegramConfigSource::File));
        assert!(persist);
    }

    #[test]
    fn incomplete_without_hash() {
        let (acc, persist) = resolve_from_layers(vec![layer(TelegramConfigSource::File, 99, "")]);
        assert!(!acc.complete());
        assert!(!persist);
    }

    #[test]
    fn reads_telegram_toml() {
        let dir = std::env::temp_dir().join(format!("tglib-tg-cfg-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("telegram.toml");
        std::fs::write(
            &path,
            "[telegram]\napi_id = 4242\napi_hash = \"deadbeefcafebabe\"\nuse_test_dc = true\n",
        )
        .unwrap();
        let layer = read_file_layer(&path).expect("parsed");
        assert_eq!(layer.api_id, Some(4242));
        assert_eq!(layer.api_hash.as_deref(), Some("deadbeefcafebabe"));
        assert_eq!(layer.use_test_dc, Some(true));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn debug_redacts_hash() {
        let cfg = TelegramClientConfig {
            api_id: 1,
            api_hash: "super-secret-hash".into(),
            use_test_dc: false,
            source: TelegramConfigSource::File,
        };
        let rendered = format!("{cfg:?}");
        assert!(rendered.contains("[redacted]"));
        assert!(!rendered.contains("super-secret-hash"));
    }
}
