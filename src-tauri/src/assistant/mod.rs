//! The assistant: an opt-in chat with a model the player connects in
//! Settings, through any OpenAI-compatible chat completions API.
//!
//! It is off by default. While it is off the UI shows no chat and the
//! shell makes no request. The endpoint and model are app settings
//! ([`AssistantSettings`]); the API key is kept in the login Keychain
//! ([`keychain`]) and read only here, in the shell, which makes every
//! request ([`provider`]). The key goes from the Settings field to the
//! Keychain once and never comes back to the web view.
//!
//! See `docs/src/content/docs/architecture/assistant.md`.

pub mod keychain;
pub mod provider;

use serde::Serialize;

use crate::settings::AssistantSettings;

/// The assistant as the UI sees it: the settings, and whether a key is
/// saved, never the key. Mirrors `AssistantStatus` in the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantStatus {
    #[serde(flatten)]
    pub settings: AssistantSettings,
    /// A key is in the Keychain.
    pub has_key: bool,
}

impl AssistantStatus {
    /// The status for these settings. The Keychain is only asked while
    /// the assistant is on.
    pub fn of(settings: &AssistantSettings) -> Self {
        Self {
            settings: settings.clone(),
            has_key: settings.enabled && keychain::has_key(),
        }
    }
}

/// Where requests go, with the key, or why none can be made.
pub fn connection(settings: &AssistantSettings) -> Result<provider::Connection, provider::Problem> {
    if !settings.enabled {
        return Err(provider::Problem::off());
    }
    let key = keychain::key().map_err(|e| provider::Problem::other(format!("{e:#}")))?;
    provider::Connection::new(&settings.endpoint, &settings.model, key)
}
