use crate::AppError;
use libsql::{params, Connection};

pub mod diff_cache;
pub mod standards_cache;

/// Create the schema on a freshly opened connection. The SQL is identical to the
/// historical rusqlite bootstrap, so an existing `pex.db` is adopted unchanged
/// whether opened locally or as a synced database.
pub async fn init_schema(conn: &Connection) -> Result<(), AppError> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS viewed_files (
            org_url TEXT NOT NULL,
            project_id TEXT NOT NULL,
            repo_id TEXT NOT NULL,
            pr_id INTEGER NOT NULL,
            file_path TEXT NOT NULL,
            viewed_at TEXT NOT NULL DEFAULT (datetime('now')),
            PRIMARY KEY (org_url, project_id, repo_id, pr_id, file_path)
        );

        CREATE TABLE IF NOT EXISTS saved_orgs (
            org_url TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            token_type TEXT NOT NULL DEFAULT 'pat',
            provider TEXT NOT NULL DEFAULT 'ado',
            added_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS pr_cache (
            org_url TEXT NOT NULL,
            project_id TEXT NOT NULL,
            repo_id TEXT NOT NULL,
            pr_id INTEGER NOT NULL,
            data TEXT NOT NULL,
            cached_at TEXT NOT NULL DEFAULT (datetime('now')),
            PRIMARY KEY (org_url, project_id, repo_id, pr_id)
        );

        CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS finding_verdicts (
            pr_key TEXT NOT NULL,
            fingerprint TEXT NOT NULL,
            verdict TEXT NOT NULL,
            file_path TEXT NOT NULL DEFAULT '',
            severity TEXT NOT NULL DEFAULT '',
            tier TEXT NOT NULL DEFAULT '',
            confidence INTEGER NOT NULL DEFAULT 0,
            comment TEXT NOT NULL DEFAULT '',
            sources TEXT NOT NULL DEFAULT '',
            updated_at TEXT NOT NULL DEFAULT (datetime('now')),
            PRIMARY KEY (pr_key, fingerprint)
        );
    ",
    )
    .await?;

    // Lightweight migration: add `sources` to finding_verdicts tables created
    // before per-specialist calibration existed. Ignored if the column is
    // already present (fresh DBs get it from the CREATE above).
    let _ = conn
        .execute(
            "ALTER TABLE finding_verdicts ADD COLUMN sources TEXT NOT NULL DEFAULT ''",
            (),
        )
        .await;

    // Lightweight migration: add `provider` to saved_orgs tables created before
    // multi-provider support. Pre-existing rows are Azure DevOps connections.
    let _ = conn
        .execute(
            "ALTER TABLE saved_orgs ADD COLUMN provider TEXT NOT NULL DEFAULT 'ado'",
            (),
        )
        .await;

    Ok(())
}

pub(crate) fn dirs_db_path() -> Result<String, AppError> {
    let data_dir = dirs_data_dir()?;
    std::fs::create_dir_all(&data_dir)
        .map_err(|e| AppError::Storage(format!("Failed to create data dir {data_dir}: {e}")))?;
    Ok(format!("{}/pex.db", data_dir))
}

/// Directory where opt-in review diagnostic traces are written
/// (`<data_dir>/diagnostics`). Created on demand.
pub fn diagnostics_dir() -> Result<String, AppError> {
    let dir = format!("{}/diagnostics", dirs_data_dir()?);
    std::fs::create_dir_all(&dir)
        .map_err(|e| AppError::Storage(format!("Failed to create diagnostics dir {dir}: {e}")))?;
    Ok(dir)
}

#[cfg(target_os = "android")]
pub(crate) fn dirs_data_dir() -> Result<String, AppError> {
    // Android has no $HOME. Use the app-private files directory, which is
    // sandboxed to this app and created by the OS at install time. We resolve
    // it from the fixed package id rather than `Context.getFilesDir()` over JNI
    // because `db::Store::open` runs before tao/wry initializes the `ndk_context`
    // Android context — calling into JNI here would panic. This is the same
    // path `getFilesDir()` returns (`/data/data/<pkg>` symlinks to the
    // per-user dir for user 0).
    Ok("/data/data/com.pex.pr_reviewer/files/pex".to_string())
}

#[cfg(not(target_os = "android"))]
pub(crate) fn dirs_data_dir() -> Result<String, AppError> {
    // Use XDG_DATA_HOME on Linux, AppData on Windows, Application Support on Apple platforms.
    if let Ok(dir) = std::env::var("XDG_DATA_HOME") {
        return Ok(format!("{}/pex", dir));
    }
    if let Ok(home) = std::env::var("HOME") {
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        return Ok(format!(
            "{}/Library/Application Support/com.pex.pr-reviewer",
            home
        ));
        #[cfg(target_os = "linux")]
        return Ok(format!("{}/.local/share/pex", home));
    }
    Err(AppError::Storage("no home dir".to_string()))
}

// ---- Viewed Files ----

pub async fn set_viewed(
    conn: &Connection,
    org: &str,
    project: &str,
    repo: &str,
    pr: i64,
    path: &str,
    viewed: bool,
) -> Result<(), AppError> {
    if viewed {
        conn.execute(
            "INSERT OR REPLACE INTO viewed_files (org_url, project_id, repo_id, pr_id, file_path) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![org, project, repo, pr, path],
        )
        .await?;
    } else {
        conn.execute(
            "DELETE FROM viewed_files WHERE org_url=?1 AND project_id=?2 AND repo_id=?3 AND pr_id=?4 AND file_path=?5",
            params![org, project, repo, pr, path],
        )
        .await?;
    }
    Ok(())
}

pub async fn get_viewed(
    conn: &Connection,
    org: &str,
    project: &str,
    repo: &str,
    pr: i64,
) -> Result<Vec<String>, AppError> {
    let mut rows = conn
        .query(
            "SELECT file_path FROM viewed_files WHERE org_url=?1 AND project_id=?2 AND repo_id=?3 AND pr_id=?4",
            params![org, project, repo, pr],
        )
        .await?;
    let mut paths = Vec::new();
    while let Some(row) = rows.next().await? {
        paths.push(row.get::<String>(0)?);
    }
    Ok(paths)
}

// ---- Saved Orgs ----

pub async fn save_org(
    conn: &Connection,
    org_url: &str,
    name: &str,
    token_type: &str,
    provider: &str,
) -> Result<(), AppError> {
    conn.execute(
        "INSERT OR REPLACE INTO saved_orgs (org_url, name, token_type, provider) VALUES (?1, ?2, ?3, ?4)",
        params![org_url, name, token_type, provider],
    )
    .await?;
    Ok(())
}

/// Returns `(org_url, name, token_type, provider)` for each saved connection.
pub async fn list_orgs(
    conn: &Connection,
) -> Result<Vec<(String, String, String, String)>, AppError> {
    let mut rows = conn
        .query(
            "SELECT org_url, name, token_type, provider FROM saved_orgs ORDER BY added_at DESC",
            (),
        )
        .await?;
    let mut orgs = Vec::new();
    while let Some(row) = rows.next().await? {
        orgs.push((
            row.get::<String>(0)?,
            row.get::<String>(1)?,
            row.get::<String>(2)?,
            row.get::<String>(3)?,
        ));
    }
    Ok(orgs)
}

pub async fn remove_org(conn: &Connection, org_url: &str) -> Result<(), AppError> {
    conn.execute(
        "DELETE FROM saved_orgs WHERE org_url=?1",
        params![org_url],
    )
    .await?;
    Ok(())
}

// ---- Settings ----

pub async fn get_setting(conn: &Connection, key: &str) -> Result<Option<String>, AppError> {
    let mut rows = conn
        .query("SELECT value FROM settings WHERE key=?1", params![key])
        .await?;
    match rows.next().await? {
        Some(row) => Ok(Some(row.get::<String>(0)?)),
        None => Ok(None),
    }
}

pub async fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<(), AppError> {
    conn.execute(
        "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
        params![key, value],
    )
    .await?;
    Ok(())
}

pub async fn delete_setting(conn: &Connection, key: &str) -> Result<(), AppError> {
    conn.execute("DELETE FROM settings WHERE key=?1", params![key])
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn mem_conn() -> Connection {
        let db = libsql::Builder::new_local(":memory:")
            .build()
            .await
            .unwrap();
        let conn = db.connect().unwrap();
        // `:memory:` is per-connection in libsql, so create the schema on the
        // same connection the test will use.
        init_schema(&conn).await.unwrap();
        conn
    }

    #[tokio::test]
    async fn settings_round_trip_and_delete() {
        let conn = mem_conn().await;
        assert_eq!(get_setting(&conn, "k").await.unwrap(), None);
        set_setting(&conn, "k", "v1").await.unwrap();
        assert_eq!(get_setting(&conn, "k").await.unwrap(), Some("v1".to_string()));
        // INSERT OR REPLACE overwrites.
        set_setting(&conn, "k", "v2").await.unwrap();
        assert_eq!(get_setting(&conn, "k").await.unwrap(), Some("v2".to_string()));
        delete_setting(&conn, "k").await.unwrap();
        assert_eq!(get_setting(&conn, "k").await.unwrap(), None);
    }

    #[tokio::test]
    async fn viewed_files_toggle() {
        let conn = mem_conn().await;
        assert!(get_viewed(&conn, "o", "p", "r", 1).await.unwrap().is_empty());
        set_viewed(&conn, "o", "p", "r", 1, "a.rs", true).await.unwrap();
        set_viewed(&conn, "o", "p", "r", 1, "b.rs", true).await.unwrap();
        let mut viewed = get_viewed(&conn, "o", "p", "r", 1).await.unwrap();
        viewed.sort();
        assert_eq!(viewed, vec!["a.rs".to_string(), "b.rs".to_string()]);
        // Scoped by PR.
        assert!(get_viewed(&conn, "o", "p", "r", 2).await.unwrap().is_empty());
        // Un-viewing removes only that row.
        set_viewed(&conn, "o", "p", "r", 1, "a.rs", false).await.unwrap();
        assert_eq!(get_viewed(&conn, "o", "p", "r", 1).await.unwrap(), vec!["b.rs".to_string()]);
    }

    #[tokio::test]
    async fn saved_orgs_upsert_and_remove() {
        let conn = mem_conn().await;
        save_org(&conn, "https://o", "Org", "pat", "ado").await.unwrap();
        save_org(&conn, "https://o", "Org Renamed", "oauth", "ado").await.unwrap();
        let orgs = list_orgs(&conn).await.unwrap();
        assert_eq!(orgs.len(), 1, "same org_url replaces");
        assert_eq!(orgs[0].1, "Org Renamed");
        assert_eq!(orgs[0].2, "oauth");
        remove_org(&conn, "https://o").await.unwrap();
        assert!(list_orgs(&conn).await.unwrap().is_empty());
    }
}
