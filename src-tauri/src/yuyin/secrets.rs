//! The LLM API key lives in the system keychain (macOS Keychain, Windows
//! Credential Manager), never in a settings file or the repository (product
//! definition, principle A). The frontend can set or clear it and ask whether
//! one exists, but can never read it back.
//!
//! On macOS the entry is a generic password (service `tw.yuyin.dictation`,
//! account `llm-api-key`), the same item the pre-1.0 builds wrote with
//! security-framework, so an existing key keeps working.
//!
//! Each clean-up service has its own key, named by the host it is sent to:
//! DeepSeek keeps the original `llm-api-key` account, any other host gets
//! `llm-api-key@<host>`. Switching services therefore never sends one
//! provider's key to another.

use std::collections::HashMap;
use std::sync::Mutex;

use once_cell::sync::Lazy;

const SERVICE: &str = "tw.yuyin.dictation";
const ACCOUNT: &str = "llm-api-key";
const DEEPSEEK_HOST: &str = "api.deepseek.com";

/// The keychain account holding `host`'s key.
fn account(host: &str) -> String {
    if host == DEEPSEEK_HOST {
        ACCOUNT.to_string()
    } else {
        format!("{ACCOUNT}@{host}")
    }
}

#[cfg(any(target_os = "macos", windows))]
mod imp {
    use super::SERVICE;
    use keyring::{Entry, Error};

    fn entry(account: &str) -> Result<Entry, String> {
        Entry::new(SERVICE, account).map_err(|e| e.to_string())
    }

    pub fn get(account: &str) -> Option<String> {
        entry(account)
            .ok()?
            .get_password()
            .ok()
            .filter(|k| !k.trim().is_empty())
    }

    pub fn set(account: &str, key: &str) -> Result<(), String> {
        entry(account)?
            .set_password(key.trim())
            .map_err(|e| e.to_string())
    }

    pub fn clear(account: &str) -> Result<(), String> {
        match entry(account)?.delete_credential() {
            Ok(()) | Err(Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
mod imp {
    pub fn get(_account: &str) -> Option<String> {
        None
    }
    pub fn set(_account: &str, _key: &str) -> Result<(), String> {
        Err("API key storage is not implemented on this platform".into())
    }
    pub fn clear(_account: &str) -> Result<(), String> {
        Ok(())
    }
}

/// Each host's key as last read from or written to the keychain; absent until
/// the first read. Without a Team ID, macOS asks for the login password once
/// per new build before handing the key over. Reading it at launch (`warm`)
/// puts that prompt there: read on a key press, the password field's secure
/// input swallowed the talk key's release and the recording never stopped.
static CACHE: Lazy<Mutex<HashMap<String, Option<String>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// Read the configured service's key once in the background so any keychain
/// prompt shows at launch. `None`: no service, nothing to read.
pub fn warm(host: Option<String>) {
    let Some(host) = host else { return };
    std::thread::spawn(move || {
        let _ = api_key(&host);
    });
}

/// The key saved for `host` (as in `polish::host`). A blank host (a custom
/// service without an address yet) has none.
pub fn api_key(host: &str) -> Option<String> {
    if host.is_empty() {
        return None;
    }
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    cache
        .entry(host.to_string())
        .or_insert_with(|| imp::get(&account(host)))
        .clone()
}

pub fn has_api_key(host: &str) -> bool {
    api_key(host).is_some()
}

/// Save `key` for `host`; an empty key removes it.
pub fn set_api_key(host: &str, key: &str) -> Result<(), String> {
    if host.is_empty() {
        return Err("set the service address first".into());
    }
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let key = key.trim();
    if key.is_empty() {
        imp::clear(&account(host))?;
        cache.insert(host.to_string(), None);
    } else {
        imp::set(&account(host), key)?;
        cache.insert(host.to_string(), Some(key.to_string()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deepseek_keeps_the_original_entry_and_others_get_their_own() {
        assert_eq!(account("api.deepseek.com"), "llm-api-key");
        assert_eq!(account("openrouter.ai"), "llm-api-key@openrouter.ai");
        assert_eq!(account("localhost:11434"), "llm-api-key@localhost:11434");
    }

    #[test]
    fn a_service_without_an_address_has_no_key() {
        assert_eq!(api_key(""), None);
        assert!(set_api_key("", "sk-test").is_err());
    }
}
