// OAuth 2.0 device-code or auth-code flow for Azure AD.
// Stub — implemented in Phase 2 after PAT auth is working.

use crate::AppError;

pub async fn start_oauth_flow(_org_url: &str) -> Result<String, AppError> {
    Err(AppError::Auth("OAuth not yet implemented — use PAT for now.".into()))
}
