use crate::AppError;
use std::collections::BTreeMap;

#[cfg(not(target_os = "android"))]
const SERVICE_NAME: &str = "pex-pr-reviewer";
const AI_CREDENTIALS_ACCOUNT: &str = "pex-ai-credentials";

/// Platform-selected secret backend.
///
/// Desktop (macOS/Windows/Linux) and iOS use the OS keyring via the `keyring`
/// crate. Android has no `keyring` backend, so it uses an AES-256-GCM encrypted
/// file whose data key is wrapped by a hardware-backed Android Keystore key
/// (see [`super::android_keystore`]).
///
/// All three primitives are keyed by `account` only (the service name is the
/// constant [`SERVICE_NAME`]). A missing secret is `Ok(None)`, never an error.
mod backend {
    #[cfg(not(target_os = "android"))]
    pub use super::keyring_backend::{delete, get, set};

    #[cfg(target_os = "android")]
    pub use crate::auth::android_keystore::{delete, get, set};
}

#[cfg(not(target_os = "android"))]
mod keyring_backend {
    use super::{AppError, SERVICE_NAME};
    use keyring::Entry;

    pub fn get(account: &str) -> Result<Option<String>, AppError> {
        match Entry::new(SERVICE_NAME, account) {
            Ok(e) => match e.get_password() {
                Ok(p) => Ok(Some(p)),
                Err(keyring::Error::NoEntry) => Ok(None),
                Err(e) => Err(AppError::Keyring(e)),
            },
            Err(e) => Err(AppError::Keyring(e)),
        }
    }

    pub fn set(account: &str, secret: &str) -> Result<(), AppError> {
        let entry = Entry::new(SERVICE_NAME, account)?;
        entry.set_password(secret)?;
        Ok(())
    }

    pub fn delete(account: &str) -> Result<(), AppError> {
        let entry = Entry::new(SERVICE_NAME, account)?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(AppError::Keyring(e)),
        }
    }
}

pub struct KeyringStore;

impl KeyringStore {
    /// Save a PAT (keyed by org URL).
    pub fn save_pat(org_url: &str, pat: &str) -> Result<(), AppError> {
        backend::set(&format!("pat:{}", org_url), pat)
    }

    /// Retrieve a PAT for an org URL.
    pub fn get_pat(org_url: &str) -> Result<Option<String>, AppError> {
        backend::get(&format!("pat:{}", org_url))
    }

    /// Delete a PAT for an org URL.
    pub fn delete_pat(org_url: &str) -> Result<(), AppError> {
        backend::delete(&format!("pat:{}", org_url))
    }

    /// Save OAuth credentials (refresh token + client secret) for an org.
    pub fn save_oauth(
        org_url: &str,
        refresh_token: &str,
        client_secret: &str,
    ) -> Result<(), AppError> {
        let data = serde_json::json!({
            "refresh_token": refresh_token,
            "client_secret": client_secret,
        });
        backend::set(&format!("oauth:{}", org_url), &data.to_string())
    }

    /// Retrieve OAuth credentials for an org.
    pub fn get_oauth(org_url: &str) -> Result<Option<(String, String)>, AppError> {
        match backend::get(&format!("oauth:{}", org_url))? {
            Some(p) => {
                let data: serde_json::Value = serde_json::from_str(&p)
                    .map_err(|e| AppError::Auth(format!("Invalid OAuth data: {}", e)))?;
                let rt = data["refresh_token"].as_str().unwrap_or("").to_string();
                let cs = data["client_secret"].as_str().unwrap_or("").to_string();
                if rt.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some((rt, cs)))
                }
            }
            None => Ok(None),
        }
    }

    /// List all saved org URLs.
    pub fn list_orgs() -> Result<Vec<(String, String)>, AppError> {
        // keyring doesn't support listing — the org list lives in SQLite via the
        // cache module. This stub exists for future platform-native enumeration.
        Ok(vec![])
    }

    /// Save a generic token by service name.
    pub fn save_token(service: &str, token: &str) -> Result<(), AppError> {
        backend::set(service, token)
    }

    /// Retrieve a generic token by service name.
    pub fn get_token(service: &str) -> Result<Option<String>, AppError> {
        backend::get(service)
    }

    /// Save an AI provider token into one bundled keychain item. Keeping all AI
    /// provider keys under one account avoids one OS prompt per provider key.
    pub fn save_ai_token(provider: &str, token: &str) -> Result<(), AppError> {
        let mut bundle = Self::get_ai_token_bundle()?;
        bundle.insert(provider.to_string(), token.to_string());
        Self::save_ai_token_bundle(&bundle)
    }

    /// Delete an AI provider token from the bundled keychain item.
    pub fn delete_ai_token(provider: &str) -> Result<(), AppError> {
        let mut bundle = Self::get_ai_token_bundle()?;
        bundle.remove(provider);
        Self::save_ai_token_bundle(&bundle)
    }

    /// Retrieve an AI provider token from the bundled keychain item.
    ///
    /// Backward compatibility: if the bundle does not contain the requested
    /// provider, try the legacy per-provider key and migrate that value into
    /// the bundle.
    pub fn get_ai_token(provider: &str) -> Result<Option<String>, AppError> {
        let mut bundle = Self::get_ai_token_bundle()?;
        if let Some(token) = bundle.get(provider).filter(|t| !t.is_empty()) {
            return Ok(Some(token.clone()));
        }

        let legacy_service = match provider {
            "openai" => "pex-ai-openai",
            "anthropic" => "pex-ai-anthropic",
            _ => return Ok(None),
        };
        if let Some(token) = Self::get_token(legacy_service)? {
            if !token.is_empty() {
                bundle.insert(provider.to_string(), token.clone());
                Self::save_ai_token_bundle(&bundle)?;
                return Ok(Some(token));
            }
        }

        Ok(None)
    }

    fn get_ai_token_bundle() -> Result<BTreeMap<String, String>, AppError> {
        match backend::get(AI_CREDENTIALS_ACCOUNT)? {
            Some(raw) => serde_json::from_str(&raw)
                .map_err(|e| AppError::Auth(format!("Invalid AI credentials bundle: {}", e))),
            None => Ok(BTreeMap::new()),
        }
    }

    fn save_ai_token_bundle(bundle: &BTreeMap<String, String>) -> Result<(), AppError> {
        let payload = serde_json::to_string(bundle)
            .map_err(|e| AppError::Auth(format!("Failed to serialize AI credentials: {}", e)))?;
        backend::set(AI_CREDENTIALS_ACCOUNT, &payload)
    }
}
