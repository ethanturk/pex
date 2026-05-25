use crate::AppError;
use std::net::TcpListener;

const AUTHORIZE_URL: &str = "https://app.vssps.visualstudio.com/oauth2/authorize";
const TOKEN_URL: &str = "https://app.vssps.visualstudio.com/oauth2/token";

/// Start the OAuth 2.0 authorization code flow for Azure DevOps.
/// Opens the system browser, waits for the redirect callback, and returns the access token.
pub async fn start_oauth_flow(
    org_url: &str,
    client_id: &str,
    client_secret: &str,
    open_url_fn: impl Fn(&str),
) -> Result<OAuthToken, AppError> {
    // 1. Bind to a random port
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|e| AppError::Auth(format!("Failed to bind callback server: {}", e)))?;
    let port = listener.local_addr().unwrap().port();
    let redirect_uri = format!("http://localhost:{}/callback", port);

    // 2. Generate state for CSRF
    let state = uuid_v4();

    // 3. Build the authorize URL
    let auth_url = format!(
        "{}?client_id={}&response_type=Assertion&state={}&scope=vso.code_write%20vso.code_status&redirect_uri={}",
        AUTHORIZE_URL,
        urlencoding(client_id),
        state,
        urlencoding(&redirect_uri),
    );

    // 4. Open browser
    open_url_fn(&auth_url);

    // 5. Wait for the callback (single request, then close)
    let (code, returned_state) = accept_callback(&listener).await?;

    if returned_state != state {
        return Err(AppError::Auth("OAuth state mismatch — possible CSRF attack".into()));
    }

    // 6. Exchange code for token
    let token = exchange_code(client_id, client_secret, &code, &redirect_uri).await?;

    // Validate the token works against the org
    validate_token(org_url, &token.access_token).await?;

    Ok(token)
}

async fn accept_callback(listener: &TcpListener) -> Result<(String, String), AppError> {
    let (mut stream, _) = tokio::net::TcpListener::from_std(listener.try_clone().unwrap())
        .map_err(|e| AppError::Auth(e.to_string()))?
        .accept()
        .await
        .map_err(|e| AppError::Auth(format!("Callback accept failed: {}", e)))?;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut buf = [0u8; 4096];
    let n = AsyncReadExt::read(&mut stream, &mut buf)
        .await
        .map_err(|e| AppError::Auth(e.to_string()))?;

    let request = String::from_utf8_lossy(&buf[..n]);

    // Send a success page
    let response = b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n\
        <html><body><h1>Authenticated!</h1><p>You can close this window.</p></body></html>";
    stream.write_all(response).await.map_err(|e| AppError::Auth(e.to_string()))?;
    stream.shutdown().await.ok();

    // Parse the authorization code and state from the request
    let first_line = request.lines().next().unwrap_or("");
    let path = first_line
        .split_whitespace()
        .nth(1)
        .unwrap_or("");

    let code = path.split("code=")
        .nth(1)
        .and_then(|s| s.split('&').next())
        .map(urlencoding_decode)
        .ok_or_else(|| AppError::Auth("No authorization code in callback".into()))?;

    let state = path.split("state=")
        .nth(1)
        .and_then(|s| s.split('&').next().or(Some(s)))
        .map(|s| s.trim())
        .map(urlencoding_decode)
        .unwrap_or_default();

    Ok((code, state))
}

async fn exchange_code(
    _client_id: &str,
    client_secret: &str,
    code: &str,
    redirect_uri: &str,
) -> Result<OAuthToken, AppError> {
    let client = reqwest::Client::new();
    let resp = client
        .post(TOKEN_URL)
        .form(&[
            ("client_assertion_type", "urn:ietf:params:oauth:client-assertion-type:jwt-bearer"),
            ("client_assertion", client_secret),
            ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
            ("assertion", code),
            ("redirect_uri", redirect_uri),
        ])
        .send()
        .await
        .map_err(|e| AppError::Auth(format!("Token exchange failed: {}", e)))?;

    let status = resp.status();
    let body = resp.text().await.map_err(|e| AppError::Auth(e.to_string()))?;

    if !status.is_success() {
        return Err(AppError::Auth(format!("Token exchange HTTP {}: {}", status, body)));
    }

    let token: OAuthTokenResponse = serde_json::from_str(&body)
        .map_err(|e| AppError::Auth(format!("Token parse error: {}", e)))?;

    Ok(OAuthToken {
        access_token: token.access_token,
        refresh_token: token.refresh_token,
        expires_in: token.expires_in,
    })
}

async fn validate_token(org_url: &str, token: &str) -> Result<(), AppError> {
    let url = format!(
        "{}/_apis/connectionData",
        org_url.trim_end_matches('/')
    );
    let client = reqwest::Client::new();
    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await
        .map_err(|e| AppError::Ado(format!("Token validation failed: {}", e)))?;

    if !resp.status().is_success() {
        return Err(AppError::Auth("Token not valid for this organization".into()));
    }

    Ok(())
}

/// Refresh an expired OAuth access token using the refresh token.
pub async fn refresh_oauth_token(
    client_secret: &str,
    refresh_token: &str,
    redirect_uri: &str,
) -> Result<OAuthToken, AppError> {
    let client = reqwest::Client::new();
    let resp = client
        .post(TOKEN_URL)
        .form(&[
            ("client_assertion_type", "urn:ietf:params:oauth:client-assertion-type:jwt-bearer"),
            ("client_assertion", client_secret),
            ("grant_type", "refresh_token"),
            ("assertion", refresh_token),
            ("redirect_uri", redirect_uri),
        ])
        .send()
        .await
        .map_err(|e| AppError::Auth(format!("Token refresh failed: {}", e)))?;

    let status = resp.status();
    let body = resp.text().await.map_err(|e| AppError::Auth(e.to_string()))?;

    if !status.is_success() {
        return Err(AppError::Auth(format!("Token refresh HTTP {}: {}", status, body)));
    }

    let token: OAuthTokenResponse = serde_json::from_str(&body)
        .map_err(|e| AppError::Auth(format!("Token parse error: {}", e)))?;

    Ok(OAuthToken {
        access_token: token.access_token,
        refresh_token: token.refresh_token,
        expires_in: token.expires_in,
    })
}

// ---- Types ----

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OAuthToken {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: i64,
}

#[derive(Debug, serde::Deserialize)]
struct OAuthTokenResponse {
    access_token: String,
    refresh_token: String,
    expires_in: i64,
}

// ---- Helpers ----

fn uuid_v4() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    format!("{:08x}-{:04x}", nanos, (nanos >> 16) & 0xffff)
}

fn urlencoding(s: &str) -> String {
    s.replace('%', "%25")
        .replace(' ', "%20")
        .replace('&', "%26")
        .replace('=', "%3D")
        .replace('?', "%3F")
        .replace('#', "%23")
        .replace(':', "%3A")
        .replace('/', "%2F")
}

fn urlencoding_decode(s: &str) -> String {
    s.replace("%20", " ")
        .replace("%25", "%")
        .replace("%26", "&")
        .replace("%3D", "=")
        .replace("%3F", "?")
        .replace("%23", "#")
        .replace("%3A", ":")
        .replace("%2F", "/")
}
