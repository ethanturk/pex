//! Commands backing the opt-in cross-device sync UI.
//!
//! Sync replicates all non-secret SQLite state to the user's own remote
//! `sqld`/Turso database. The remote URL + enabled flag persist locally in
//! `sync.json`; the auth token is a secret and lives in the keyring. Toggling
//! sync re-opens the same `pex.db` in the other mode, so no data is migrated or
//! lost — enabling simply adopts the existing local DB and pushes it up.

use crate::db::{self, SyncConfig, SyncStatus};
use crate::AppState;
use tauri::State;

/// Current sync status for the Settings UI (enabled, URL, last sync, errors).
#[tauri::command]
pub async fn get_sync_status(state: State<'_, AppState>) -> Result<SyncStatus, String> {
    Ok(state.db.status())
}

/// Enable sync against `url` with `token`, persisting both (token → keyring) and
/// re-opening the DB as a synced database. An initial reconcile runs inside
/// `reconfigure`, so saved orgs / settings / verdicts from other devices appear
/// once this returns.
#[tauri::command]
pub async fn enable_sync(
    state: State<'_, AppState>,
    url: String,
    token: String,
) -> Result<SyncStatus, String> {
    let url = url.trim().to_string();
    if url.is_empty() {
        return Err("A remote database URL is required.".to_string());
    }
    // An empty token means "keep the existing stored token" — the UI never
    // echoes the real token back, so blank = unchanged.
    let token = if token.trim().is_empty() {
        crate::auth::keyring_store::KeyringStore::get_token(db::SYNC_TOKEN_ACCOUNT)
            .map_err(|e| e.to_string())?
            .unwrap_or_default()
    } else {
        token
    };
    if token.is_empty() {
        return Err("An auth token is required.".to_string());
    }

    let cfg = SyncConfig {
        url,
        enabled: true,
        token,
    };
    db::save_sync_config(&cfg).map_err(|e| e.to_string())?;
    state
        .db
        .reconfigure(Some(cfg))
        .await
        .map_err(|e| e.to_string())?;
    Ok(state.db.status())
}

/// Disable sync and re-open the DB locally. The remote URL is remembered (so the
/// user can re-enable without re-typing it) but the enabled flag is cleared; the
/// local `pex.db` keeps working exactly as before.
#[tauri::command]
pub async fn disable_sync(state: State<'_, AppState>) -> Result<SyncStatus, String> {
    // Preserve the configured URL while flipping enabled off.
    let mut cfg = db::load_sync_config().unwrap_or_default();
    cfg.enabled = false;
    db::save_sync_config(&cfg).map_err(|e| e.to_string())?;
    state.db.reconfigure(None).await.map_err(|e| e.to_string())?;
    Ok(state.db.status())
}

/// Force an immediate reconcile with the remote. No-op when sync is disabled.
#[tauri::command]
pub async fn sync_now(state: State<'_, AppState>) -> Result<SyncStatus, String> {
    state.db.sync_now().await.map_err(|e| e.to_string())?;
    Ok(state.db.status())
}
