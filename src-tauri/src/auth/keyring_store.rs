use crate::AppError;
use keyring::Entry;
use std::collections::BTreeMap;

const SERVICE_NAME: &str = "pex-pr-reviewer";
const AI_CREDENTIALS_ACCOUNT: &str = "pex-ai-credentials";

pub struct KeyringStore;

impl KeyringStore {
    /// Save a PAT (keyed by org URL).
    pub fn save_pat(org_url: &str, pat: &str) -> Result<(), AppError> {
        let entry = Entry::new(SERVICE_NAME, &format!("pat:{}", org_url))?;
        entry.set_password(pat)?;
        Ok(())
    }

    /// Retrieve a PAT for an org URL.
    pub fn get_pat(org_url: &str) -> Result<Option<String>, AppError> {
        let entry = Entry::new(SERVICE_NAME, &format!("pat:{}", org_url));
        match entry {
            Ok(e) => match e.get_password() {
                Ok(p) => Ok(Some(p)),
                Err(keyring::Error::NoEntry) => Ok(None),
                Err(e) => Err(AppError::Keyring(e)),
            },
            Err(e) => Err(AppError::Keyring(e)),
        }
    }

    /// Delete a PAT for an org URL.
    pub fn delete_pat(org_url: &str) -> Result<(), AppError> {
        let entry = Entry::new(SERVICE_NAME, &format!("pat:{}", org_url))?;
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(AppError::Keyring(e)),
        }
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
        let entry = Entry::new(SERVICE_NAME, &format!("oauth:{}", org_url))?;
        entry.set_password(&data.to_string())?;
        Ok(())
    }

    /// Retrieve OAuth credentials for an org.
    pub fn get_oauth(org_url: &str) -> Result<Option<(String, String)>, AppError> {
        let entry = Entry::new(SERVICE_NAME, &format!("oauth:{}", org_url));
        match entry {
            Ok(e) => match e.get_password() {
                Ok(p) => {
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
                Err(keyring::Error::NoEntry) => Ok(None),
                Err(e) => Err(AppError::Keyring(e)),
            },
            Err(e) => Err(AppError::Keyring(e)),
        }
    }

    /// List all saved org URLs.
    pub fn list_orgs() -> Result<Vec<(String, String)>, AppError> {
        // keyring doesn't support listing — fall back to search
        // For now, we store the org list in SQLite via the cache module.
        // This stub exists for future platform-native enumeration.
        Ok(vec![])
    }

    /// Save a generic token by service name.
    pub fn save_token(service: &str, token: &str) -> Result<(), AppError> {
        let entry = Entry::new(SERVICE_NAME, service)?;
        entry.set_password(token)?;
        Ok(())
    }

    /// Retrieve a generic token by service name.
    pub fn get_token(service: &str) -> Result<Option<String>, AppError> {
        let entry = Entry::new(SERVICE_NAME, service);
        match entry {
            Ok(e) => match e.get_password() {
                Ok(p) => Ok(Some(p)),
                Err(keyring::Error::NoEntry) => Ok(None),
                Err(e) => Err(AppError::Keyring(e)),
            },
            Err(e) => Err(AppError::Keyring(e)),
        }
    }

    /// Save an AI provider token into one bundled keychain item. Keeping all AI
    /// provider keys under one account avoids one OS prompt per provider key.
    pub fn save_ai_token(provider: &str, token: &str) -> Result<(), AppError> {
        let mut bundle = Self::get_ai_token_bundle()?;
        bundle.insert(provider.to_string(), token.to_string());
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
        let entry = Entry::new(SERVICE_NAME, AI_CREDENTIALS_ACCOUNT);
        match entry {
            Ok(e) => match e.get_password() {
                Ok(raw) => serde_json::from_str(&raw)
                    .map_err(|e| AppError::Auth(format!("Invalid AI credentials bundle: {}", e))),
                Err(keyring::Error::NoEntry) => Ok(BTreeMap::new()),
                Err(e) => Err(AppError::Keyring(e)),
            },
            Err(e) => Err(AppError::Keyring(e)),
        }
    }

    fn save_ai_token_bundle(bundle: &BTreeMap<String, String>) -> Result<(), AppError> {
        let entry = Entry::new(SERVICE_NAME, AI_CREDENTIALS_ACCOUNT)?;
        let payload = serde_json::to_string(bundle)
            .map_err(|e| AppError::Auth(format!("Failed to serialize AI credentials: {}", e)))?;
        entry.set_password(&payload)?;
        Ok(())
    }
}
