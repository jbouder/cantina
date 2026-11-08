//! The assistant's API key, in the login Keychain as a generic password.
//!
//! The key is read here and nowhere else, cached in memory after the
//! first read so a request does not ask the Keychain each time. It never
//! goes into the settings file, a project, a log or the web view.

use std::sync::Mutex;

use anyhow::{Context, Result};
use security_framework::passwords;

/// The Keychain item's service; it shows under this name in Keychain Access.
const SERVICE: &str = "com.jbouder.cantina.assistant";
/// The item's account.
const ACCOUNT: &str = "api-key";
/// `errSecItemNotFound`.
const NOT_FOUND: i32 = -25300;

/// The key as last read or written: `None` until the Keychain is first
/// asked, then whether there is one.
static CACHE: Mutex<Option<Option<String>>> = Mutex::new(None);

/// The saved key, if there is one.
pub fn key() -> Result<Option<String>> {
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(key) = cache.as_ref() {
        return Ok(key.clone());
    }
    let key = match passwords::get_generic_password(SERVICE, ACCOUNT) {
        Ok(bytes) => Some(String::from_utf8(bytes).context("the saved API key is not text")?),
        Err(e) if e.code() == NOT_FOUND => None,
        Err(e) => return Err(e).context("reading the API key from the Keychain"),
    };
    *cache = Some(key.clone());
    Ok(key)
}

/// True when a key is saved. A Keychain that cannot be read counts as none.
pub fn has_key() -> bool {
    key().ok().flatten().is_some()
}

/// Saves the key, replacing any before it, or removes it when empty.
pub fn set_key(key: &str) -> Result<()> {
    let key = key.trim();
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if key.is_empty() {
        match passwords::delete_generic_password(SERVICE, ACCOUNT) {
            Ok(()) => {}
            Err(e) if e.code() == NOT_FOUND => {}
            Err(e) => return Err(e).context("removing the API key from the Keychain"),
        }
        *cache = Some(None);
    } else {
        passwords::set_generic_password(SERVICE, ACCOUNT, key.as_bytes())
            .context("saving the API key in the Keychain")?;
        *cache = Some(Some(key.to_string()));
    }
    Ok(())
}
