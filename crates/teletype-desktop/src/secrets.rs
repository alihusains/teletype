//! OS keychain storage for API keys (P0.3).
//!
//! Keys never touch `settings.json` or logs. Service name matches the app
//! bundle id so Keychain entries are grouped under Teletype.

use keyring::Entry;

pub const SERVICE: &str = "com.teletype.app";

/// Keychain account for a provider secret, e.g. `llm:openai`.
fn account(provider_id: &str) -> String {
    format!("llm:{provider_id}")
}

/// Stores `secret` for `provider_id`. Empty secrets are rejected.
pub fn set_secret(provider_id: &str, secret: &str) -> Result<(), String> {
    let secret = secret.trim();
    if secret.is_empty() {
        return Err("API key is empty".into());
    }
    let entry = Entry::new(SERVICE, &account(provider_id))
        .map_err(|e| format!("keychain entry: {e}"))?;
    entry
        .set_password(secret)
        .map_err(|e| format!("keychain set: {e}"))
}

/// Returns whether a secret exists (without reading it into logs/IPC).
pub fn has_secret(provider_id: &str) -> bool {
    Entry::new(SERVICE, &account(provider_id))
        .and_then(|e| e.get_password())
        .map(|p| !p.is_empty())
        .unwrap_or(false)
}

/// Reads the secret. Only the desktop layer should call this when building
/// a provider; it must never be returned to the UI or logged.
pub fn get_secret(provider_id: &str) -> Result<Option<String>, String> {
    match Entry::new(SERVICE, &account(provider_id)).and_then(|e| e.get_password()) {
        Ok(p) if p.is_empty() => Ok(None),
        Ok(p) => Ok(Some(p)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(format!("keychain get: {e}")),
    }
}

/// Removes the secret if present. Missing entries are not an error.
pub fn clear_secret(provider_id: &str) -> Result<(), String> {
    let entry = Entry::new(SERVICE, &account(provider_id))
        .map_err(|e| format!("keychain entry: {e}"))?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("keychain delete: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_get_clear_roundtrip() {
        let id = "test-roundtrip-provider";
        clear_secret(id).ok();
        assert!(!has_secret(id));
        set_secret(id, "sk-test-value").unwrap();
        assert!(has_secret(id));
        let got = get_secret(id).unwrap();
        assert_eq!(got.as_deref(), Some("sk-test-value"));
        clear_secret(id).unwrap();
        assert!(!has_secret(id));
        assert!(get_secret(id).unwrap().is_none());
    }

    #[test]
    fn empty_secret_rejected() {
        assert!(set_secret("test-empty", "   ").is_err());
    }
}
