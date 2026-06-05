//! The local storage engine + optional cross-device sync.
//!
//! Pex stores all non-secret state in a single SQLite file (`pex.db`) opened
//! through **libsql**. With sync **off** the file is opened locally and behaves
//! exactly as it always has. With sync **on** the same file is opened as a
//! libsql *synced database*: writes still happen locally (offline-first) and are
//! reconciled bidirectionally with the user's remote `sqld`/Turso database via
//! [`Store::sync_now`]. Because both modes use the same on-disk file, enabling
//! sync later simply adopts the existing local DB and pushes it up — there is no
//! migration/export step.
//!
//! Secrets never live here: the sync **auth token** is stored in the OS keyring
//! (account [`SYNC_TOKEN_ACCOUNT`]), and the non-secret part of the config
//! (remote URL + enabled flag) lives in a local-only `sync.json` next to
//! `pex.db`. This keeps the sync credential out of the synced DB and off the
//! device-roaming path, consistent with the rest of the secrets boundary.

use crate::AppError;
use libsql::{Builder, Connection, Database};
use std::sync::{Arc, RwLock};

/// Keyring account under which the sync auth token is stored. Kept separate from
/// the synced data so the credential never roams between devices.
pub const SYNC_TOKEN_ACCOUNT: &str = "sync:auth_token";

/// How often the background task reconciles with the remote when sync is on.
pub const DEFAULT_SYNC_INTERVAL_SECS: u64 = 60;

/// Non-secret sync configuration. Persisted to `sync.json`; the `token` is loaded
/// from / saved to the keyring separately and is never written to the file.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct SyncConfig {
    /// The remote `sqld`/Turso URL (e.g. `libsql://my-db.turso.io`).
    pub url: String,
    /// Whether sync is currently enabled.
    #[serde(default)]
    pub enabled: bool,
    /// Auth token for the remote. Loaded from the keyring at runtime; never
    /// serialized into `sync.json`.
    #[serde(skip)]
    pub token: String,
}

/// Sync state surfaced to the UI. Serialized for the `get_sync_status` command.
#[derive(Clone, Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatus {
    /// Whether sync is enabled and the DB was opened as a synced database.
    pub enabled: bool,
    /// The configured remote URL (may be set even while disabled).
    pub url: String,
    /// Whether a remote URL has been configured at all.
    pub configured: bool,
    /// True while a `sync()` is in flight.
    pub syncing: bool,
    /// RFC3339 timestamp of the last successful sync, if any.
    pub last_sync: Option<String>,
    /// Human-readable last error, cleared on the next success.
    pub last_error: Option<String>,
    /// Frames reconciled on the last successful sync.
    pub frames_synced: Option<u64>,
}

struct Inner {
    /// `Arc` so [`Store::sync_now`] can take a handle and call `sync().await`
    /// without holding the `RwLock` across the await.
    db: Arc<Database>,
    conn: Connection,
    /// True when `db` was opened via `new_synced_database` (sync is live).
    synced: bool,
}

/// The application's storage handle. Cheap to share; `conn()` hands out cloned
/// connections (libsql connections are internally synchronized).
#[derive(Clone)]
pub struct Store {
    inner: Arc<RwLock<Inner>>,
    status: Arc<RwLock<SyncStatus>>,
}

impl Store {
    /// Open the local DB, optionally as a synced database. Runs the schema
    /// bootstrap and, when sync is enabled, performs an initial reconcile so a
    /// brand-new device pulls existing state before the UI reads it.
    pub async fn open(cfg: Option<SyncConfig>) -> Result<Store, AppError> {
        let configured = cfg.as_ref().map(|c| !c.url.is_empty()).unwrap_or(false);
        let url = cfg.as_ref().map(|c| c.url.clone()).unwrap_or_default();
        let wants_sync = cfg
            .as_ref()
            .map(|c| c.enabled && !c.url.is_empty())
            .unwrap_or(false);

        // A sync failure at startup (e.g. the network is down, or the remote
        // rejects the token) must never brick the app: fall back to local-only
        // and surface the error in the status panel instead of panicking.
        let mut last_error = None;
        let (db, conn, synced) = match open_database(cfg.clone()).await {
            Ok(opened) => opened,
            Err(e) if wants_sync => {
                last_error = Some(e.to_string());
                open_database(None).await?
            }
            Err(e) => return Err(e),
        };
        crate::cache::init_schema(&conn).await?;

        let status = SyncStatus {
            enabled: synced,
            url,
            configured,
            last_error,
            ..Default::default()
        };
        let store = Store {
            inner: Arc::new(RwLock::new(Inner {
                db: Arc::new(db),
                conn,
                synced,
            })),
            status: Arc::new(RwLock::new(status)),
        };

        // Initial pull so a fresh device adopts existing remote state up front.
        if synced {
            let _ = store.sync_now().await;
        }
        Ok(store)
    }

    /// A connection handle for issuing queries. Cloned per call; libsql
    /// connections are cheap to clone and internally synchronized, so there is
    /// no external lock to hold across awaits.
    pub fn conn(&self) -> Connection {
        self.inner.read().expect("db lock poisoned").conn.clone()
    }

    /// Whether the DB is currently opened as a synced database.
    pub fn is_synced(&self) -> bool {
        self.inner.read().expect("db lock poisoned").synced
    }

    /// A snapshot of the current sync status for the UI.
    pub fn status(&self) -> SyncStatus {
        self.status.read().expect("status lock poisoned").clone()
    }

    /// Reconcile with the remote now. No-op (Ok) when sync is disabled. Updates
    /// the status snapshot with timing, frame count, and any error.
    pub async fn sync_now(&self) -> Result<(), AppError> {
        let db = {
            let inner = self.inner.read().expect("db lock poisoned");
            if !inner.synced {
                return Ok(());
            }
            inner.db.clone()
        };

        {
            let mut s = self.status.write().expect("status lock poisoned");
            s.syncing = true;
        }

        let result = db.sync().await;

        let mut s = self.status.write().expect("status lock poisoned");
        s.syncing = false;
        match result {
            Ok(replicated) => {
                s.last_sync = Some(now_rfc3339());
                s.last_error = None;
                s.frames_synced = Some(replicated.frames_synced() as u64);
                Ok(())
            }
            Err(e) => {
                s.last_error = Some(e.to_string());
                Err(AppError::Db(e))
            }
        }
    }

    /// Re-open the database under a new configuration (toggling sync on/off or
    /// pointing at a different remote). The new config is persisted first so a
    /// restart picks it up; the live DB handle is then swapped in place.
    pub async fn reconfigure(&self, cfg: Option<SyncConfig>) -> Result<(), AppError> {
        let configured = cfg.as_ref().map(|c| !c.url.is_empty()).unwrap_or(false);
        let url = cfg.as_ref().map(|c| c.url.clone()).unwrap_or_default();
        let (db, conn, synced) = open_database(cfg).await?;
        crate::cache::init_schema(&conn).await?;

        {
            let mut inner = self.inner.write().expect("db lock poisoned");
            inner.db = Arc::new(db);
            inner.conn = conn;
            inner.synced = synced;
        }
        {
            let mut s = self.status.write().expect("status lock poisoned");
            s.enabled = synced;
            s.url = url;
            s.configured = configured;
            s.last_error = None;
            s.frames_synced = None;
        }

        if synced {
            let _ = self.sync_now().await;
        }
        Ok(())
    }
}

/// Open the underlying libsql database for the given config. Returns the
/// database, a connection, and whether it was opened as a synced database.
async fn open_database(cfg: Option<SyncConfig>) -> Result<(Database, Connection, bool), AppError> {
    let path = crate::cache::dirs_db_path()?;
    match cfg {
        Some(cfg) if cfg.enabled && !cfg.url.is_empty() => {
            // The libsql sync engine only accepts two on-disk states: both
            // `pex.db` and its `pex.db-info` sidecar present (resume), or neither
            // present (bootstrap from remote). The first time sync is enabled we
            // have a plain `pex.db` (from `new_local`) but no `-info`, which it
            // rejects. Adopt the existing data instead of losing it: snapshot the
            // local rows, move the plain files aside so the synced DB can
            // bootstrap from the (possibly empty) remote, then re-import — the
            // imported rows push up on the next sync.
            let info_path = format!("{path}-info");
            let needs_adopt = std::path::Path::new(&path).exists()
                && !std::path::Path::new(&info_path).exists();

            if !needs_adopt {
                let db = build_synced_database(&path, cfg.url.clone(), cfg.token.clone()).await?;
                let conn = db.connect()?;
                return Ok((db, conn, true));
            }

            // Read all rows out of the existing local DB, then drop the handle so
            // the file can be moved.
            let snapshot = {
                let db = Builder::new_local(&path).build().await?;
                let conn = db.connect()?;
                export_all_tables(&conn).await?
            };

            stash_local(&path, BACKUP_SUFFIX)?;
            match adopt_into_synced(&path, &cfg, &snapshot).await {
                Ok((db, conn)) => {
                    drop_stash(&path, BACKUP_SUFFIX);
                    Ok((db, conn, true))
                }
                Err(e) => {
                    // Roll back so no local data is lost: remove any partial
                    // synced artifacts and restore the original files in place.
                    let _ = std::fs::remove_file(&info_path);
                    let _ = std::fs::remove_file(&path);
                    unstash_local(&path, BACKUP_SUFFIX);
                    Err(e)
                }
            }
        }
        _ => {
            let db = Builder::new_local(&path).build().await?;
            let conn = db.connect()?;
            Ok((db, conn, false))
        }
    }
}

/// One captured table: its name, column names, and rows (each a positional list
/// of values aligned to `columns`).
type TableSnapshot = (String, Vec<String>, Vec<Vec<libsql::Value>>);

/// Suffix appended to the plain DB files while they are stashed during adoption.
const BACKUP_SUFFIX: &str = ".pre-sync.bak";
/// The SQLite file and its WAL/SHM sidecars, moved/cleaned as a set.
const DB_SIDECARS: [&str; 3] = ["", "-wal", "-shm"];

/// Build the synced DB, create the schema, and import the captured rows.
async fn adopt_into_synced(
    path: &str,
    cfg: &SyncConfig,
    snapshot: &[TableSnapshot],
) -> Result<(Database, Connection), AppError> {
    let db = build_synced_database(path, cfg.url.clone(), cfg.token.clone()).await?;
    let conn = db.connect()?;
    crate::cache::init_schema(&conn).await?;
    import_snapshot(&conn, snapshot).await?;
    Ok((db, conn))
}

/// Build a libsql synced-database handle for the given remote.
///
/// On Android/iOS the libsql-bundled TLS connector fails: it loads trust roots
/// via `rustls-native-certs`, which finds no readable cert store on those
/// platforms ("no valid native root CA certificates"). There we supply our own
/// HTTPS connector backed by the compiled-in Mozilla **webpki roots**, which
/// needs no OS trust store. Desktop keeps libsql's default (native-roots)
/// connector unchanged, so corporate/self-hosted CAs keep working there.
async fn build_synced_database(
    path: &str,
    url: String,
    token: String,
) -> Result<Database, AppError> {
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        // Same hyper/rustls types libsql uses internally (pinned in Cargo.toml),
        // so this connector satisfies libsql's `Socket` bound.
        let connector = hyper_rustls::HttpsConnectorBuilder::new()
            .with_webpki_roots()
            .https_or_http()
            .enable_http1()
            .enable_http2()
            .build();
        let db = Builder::new_synced_database(path, url, token)
            .connector(connector)
            .build()
            .await?;
        Ok(db)
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        let db = Builder::new_synced_database(path, url, token).build().await?;
        Ok(db)
    }
}

/// Read every user table (name + columns + rows) from a connection. Internal
/// SQLite/libsql bookkeeping tables are skipped.
async fn export_all_tables(conn: &Connection) -> Result<Vec<TableSnapshot>, AppError> {
    let mut names = Vec::new();
    let mut name_rows = conn
        .query(
            "SELECT name FROM sqlite_master WHERE type='table' \
             AND name NOT LIKE 'sqlite_%' AND name NOT LIKE 'libsql_%'",
            (),
        )
        .await?;
    while let Some(row) = name_rows.next().await? {
        names.push(row.get::<String>(0)?);
    }

    let mut out = Vec::with_capacity(names.len());
    for name in names {
        let mut rows = conn.query(&format!("SELECT * FROM \"{name}\""), ()).await?;
        let ncol = rows.column_count();
        let columns: Vec<String> = (0..ncol)
            .map(|i| rows.column_name(i).unwrap_or_default().to_string())
            .collect();
        let mut table_rows = Vec::new();
        while let Some(row) = rows.next().await? {
            let mut vals = Vec::with_capacity(ncol as usize);
            for i in 0..ncol {
                vals.push(row.get_value(i)?);
            }
            table_rows.push(vals);
        }
        out.push((name, columns, table_rows));
    }
    Ok(out)
}

/// Insert captured rows into the (freshly bootstrapped) synced DB. Inserts are
/// keyed by **column name**, not position, because an older local DB may have
/// gained columns via `ALTER TABLE` (appended last) while the fresh schema
/// declares them mid-table — a positional insert would misalign them.
async fn import_snapshot(conn: &Connection, snapshot: &[TableSnapshot]) -> Result<(), AppError> {
    for (table, columns, rows) in snapshot {
        if columns.is_empty() {
            continue;
        }
        let collist = columns
            .iter()
            .map(|c| format!("\"{c}\""))
            .collect::<Vec<_>>()
            .join(", ");
        let placeholders = (1..=columns.len())
            .map(|i| format!("?{i}"))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!("INSERT OR REPLACE INTO \"{table}\" ({collist}) VALUES ({placeholders})");
        for row in rows {
            conn.execute(&sql, row.clone()).await?;
        }
    }
    Ok(())
}

/// Move the SQLite file (and WAL/SHM sidecars) aside, appending `suffix`.
fn stash_local(path: &str, suffix: &str) -> Result<(), AppError> {
    for s in DB_SIDECARS {
        let from = format!("{path}{s}");
        if std::path::Path::new(&from).exists() {
            let to = format!("{path}{s}{suffix}");
            std::fs::rename(&from, &to)
                .map_err(|e| AppError::Storage(format!("Failed to stash {from}: {e}")))?;
        }
    }
    Ok(())
}

/// Restore stashed files back into place (best effort, used on rollback).
fn unstash_local(path: &str, suffix: &str) {
    for s in DB_SIDECARS {
        let from = format!("{path}{s}{suffix}");
        if std::path::Path::new(&from).exists() {
            let _ = std::fs::rename(&from, format!("{path}{s}"));
        }
    }
}

/// Delete stashed files after a successful adoption.
fn drop_stash(path: &str, suffix: &str) {
    for s in DB_SIDECARS {
        let _ = std::fs::remove_file(format!("{path}{s}{suffix}"));
    }
}

/// Path to the local-only `sync.json` config file (next to `pex.db`).
fn sync_config_path() -> Result<String, AppError> {
    Ok(format!("{}/sync.json", crate::cache::dirs_data_dir()?))
}

/// Load the persisted sync config: the non-secret part from `sync.json` and the
/// auth token from the keyring. Returns `None` when nothing has been configured.
pub fn load_sync_config() -> Option<SyncConfig> {
    let path = sync_config_path().ok()?;
    let raw = std::fs::read_to_string(&path).ok()?;
    let mut cfg: SyncConfig = serde_json::from_str(&raw).ok()?;
    cfg.token = crate::auth::keyring_store::KeyringStore::get_token(SYNC_TOKEN_ACCOUNT)
        .ok()
        .flatten()
        .unwrap_or_default();
    Some(cfg)
}

/// Persist the sync config: the URL + enabled flag to `sync.json`, the token to
/// the keyring. The token is never written to the file.
pub fn save_sync_config(cfg: &SyncConfig) -> Result<(), AppError> {
    let path = sync_config_path()?;
    let json = serde_json::to_string_pretty(cfg)
        .map_err(|e| AppError::Storage(format!("Failed to serialize sync config: {e}")))?;
    std::fs::write(&path, json)
        .map_err(|e| AppError::Storage(format!("Failed to write {path}: {e}")))?;
    if cfg.token.is_empty() {
        let _ = crate::auth::keyring_store::KeyringStore::delete_token(SYNC_TOKEN_ACCOUNT);
    } else {
        crate::auth::keyring_store::KeyringStore::save_token(SYNC_TOKEN_ACCOUNT, &cfg.token)?;
    }
    Ok(())
}

/// Delete the persisted sync config and token (full opt-out).
pub fn clear_sync_config() -> Result<(), AppError> {
    if let Ok(path) = sync_config_path() {
        let _ = std::fs::remove_file(path);
    }
    let _ = crate::auth::keyring_store::KeyringStore::delete_token(SYNC_TOKEN_ACCOUNT);
    Ok(())
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn mem() -> Connection {
        let db = Builder::new_local(":memory:").build().await.unwrap();
        db.connect().unwrap()
    }

    #[tokio::test]
    async fn snapshot_round_trip_maps_columns_by_name() {
        // Source mimics an OLD local DB: `finding_verdicts` created before the
        // `sources` column existed, with `sources` appended last via ALTER — so
        // its column order (…, updated_at, sources) differs from the fresh schema
        // (…, sources, updated_at). A positional copy would swap the two; the
        // name-based import must not.
        let src = mem().await;
        src.execute_batch(
            "CREATE TABLE finding_verdicts (
                pr_key TEXT NOT NULL, fingerprint TEXT NOT NULL, verdict TEXT NOT NULL,
                file_path TEXT NOT NULL DEFAULT '', severity TEXT NOT NULL DEFAULT '',
                tier TEXT NOT NULL DEFAULT '', confidence INTEGER NOT NULL DEFAULT 0,
                comment TEXT NOT NULL DEFAULT '',
                updated_at TEXT NOT NULL DEFAULT '',
                PRIMARY KEY (pr_key, fingerprint));
             ALTER TABLE finding_verdicts ADD COLUMN sources TEXT NOT NULL DEFAULT '';
             CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
        )
        .await
        .unwrap();
        src.execute(
            "INSERT INTO finding_verdicts
                (pr_key, fingerprint, verdict, file_path, severity, tier, confidence, comment, updated_at, sources)
             VALUES ('pr', 'fp', 'dismissed', 'a.rs', 'minor', 'nit', 80, 'noisy', '2026-01-01', 'code-reviewer')",
            (),
        )
        .await
        .unwrap();
        crate::cache::set_setting(&src, "ai_model", "gpt-x").await.unwrap();

        let snapshot = export_all_tables(&src).await.unwrap();

        // Destination uses the current schema (where `sources` precedes `updated_at`).
        let dst = mem().await;
        crate::cache::init_schema(&dst).await.unwrap();
        import_snapshot(&dst, &snapshot).await.unwrap();

        // Settings carried over.
        assert_eq!(
            crate::cache::get_setting(&dst, "ai_model").await.unwrap(),
            Some("gpt-x".to_string())
        );
        // Columns landed by name despite the order mismatch.
        let mut rows = dst
            .query(
                "SELECT sources, updated_at, verdict FROM finding_verdicts WHERE pr_key='pr'",
                (),
            )
            .await
            .unwrap();
        let row = rows.next().await.unwrap().unwrap();
        assert_eq!(row.get::<String>(0).unwrap(), "code-reviewer");
        assert_eq!(row.get::<String>(1).unwrap(), "2026-01-01");
        assert_eq!(row.get::<String>(2).unwrap(), "dismissed");
        // And the higher-level suppression query reads it back correctly.
        let dismissed = crate::review::feedback::dismissed_fingerprints(&dst, "pr")
            .await
            .unwrap();
        assert!(dismissed.contains("fp"));
    }
}

/// Spawn the background reconcile loop. It wakes every
/// [`DEFAULT_SYNC_INTERVAL_SECS`] and syncs when sync is enabled; a no-op
/// otherwise, so it is safe to spawn unconditionally at startup.
pub fn spawn_background_sync(store: Store) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(DEFAULT_SYNC_INTERVAL_SECS)).await;
            if store.is_synced() {
                let _ = store.sync_now().await;
            }
        }
    });
}
