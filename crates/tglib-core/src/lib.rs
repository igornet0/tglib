//! tglib Telegram user-account domain.
//!
//! Isolated from Telegram Bot API types and from TDLib FFI types.

mod account;
mod audit;
mod error;
mod events;
mod ids;
mod permissions;

pub use account::*;
pub use audit::*;
pub use error::*;
pub use events::*;
pub use ids::*;
pub use permissions::*;

/// Shown when a user connects a Telegram account. Never hide this.
pub const ACCOUNT_CONSENT: &str =
    "This Telegram account will be controlled by the application according to the automations you enable.";

/// Structured status marker for library / integration self-checks.
pub const TELEGRAM_ENGINE_STATUS: &str = "READY";

/// Mask a phone number for storage, API, and logs. Never log the raw input.
pub fn mask_phone(phone: &str) -> String {
    let trimmed: String = phone.chars().filter(|c| !c.is_whitespace()).collect();
    if trimmed.len() <= 4 {
        return "****".into();
    }
    let prefix_len = if trimmed.starts_with('+') { 3 } else { 2 };
    let prefix: String = trimmed.chars().take(prefix_len).collect();
    let suffix: String = trimmed.chars().rev().take(2).collect::<String>().chars().rev().collect();
    format!("{prefix} *** *** {suffix}")
}
