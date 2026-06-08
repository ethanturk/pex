//! GitHub Personal Access Token validation.

use crate::AppError;

/// Validate a GitHub PAT against `{api_base}/user`. Returns the authenticated
/// user's login on success. `api_base` is the REST base (e.g.
/// `https://api.github.com` or `https://HOST/api/v3`).
pub async fn validate_github_pat(api_base: &str, pat: &str) -> Result<String, AppError> {
    let url = format!("{}/user", api_base.trim_end_matches('/'));
    let client = reqwest::Client::new();
    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", pat))
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "pex")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .await?;

    let status = resp.status();
    let body = resp
        .text()
        .await
        .map_err(|e| AppError::Auth(e.to_string()))?;
    if !status.is_success() {
        return Err(AppError::Auth(format!(
            "GitHub authentication failed ({}): {}",
            status, body
        )));
    }

    let json: serde_json::Value =
        serde_json::from_str(&body).map_err(|e| AppError::Auth(e.to_string()))?;
    json.get("login")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| AppError::Auth("GitHub /user response missing login".to_string()))
}
