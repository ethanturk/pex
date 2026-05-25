use crate::auth::keyring_store::KeyringStore;
use crate::AppError;

/// Validates a PAT against the Azure DevOps API.
/// Returns the user's display name on success.
pub async fn validate_pat(org_url: &str, pat: &str) -> Result<String, AppError> {
    let url = format!("{}/_apis/connectionData", org_url.trim_end_matches('/'));
    let client = reqwest::Client::new();
    let auth_header = format!(
        "Basic {}",
        base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            format!(":{}", pat),
        )
    );

    let resp = client
        .get(&url)
        .header("Authorization", auth_header)
        .send()
        .await
        .map_err(|e| AppError::Ado(format!("Connection failed: {}", e)))?;

    if !resp.status().is_success() {
        return Err(AppError::Ado(format!(
            "Invalid credentials (HTTP {})",
            resp.status()
        )));
    }

    let data: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| AppError::Ado(e.to_string()))?;
    let name = data["authenticatedUser"]["providerDisplayName"]
        .as_str()
        .unwrap_or("Unknown")
        .to_string();

    // Only persist on successful validation
    KeyringStore::save_pat(org_url, pat)?;

    Ok(name)
}
