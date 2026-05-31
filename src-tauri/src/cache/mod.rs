use crate::AppError;
use rusqlite::Connection;

pub mod diff_cache;
pub mod standards_cache;

/// Initialize the SQLite database and return a connection.
pub fn init_db() -> Result<Connection, AppError> {
    let db_path = dirs_db_path()?;
    let conn = Connection::open(&db_path)?;

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
    )?;

    // Lightweight migration: add `sources` to finding_verdicts tables created
    // before per-specialist calibration existed. Ignored if the column is
    // already present (fresh DBs get it from the CREATE above).
    let _ = conn.execute(
        "ALTER TABLE finding_verdicts ADD COLUMN sources TEXT NOT NULL DEFAULT ''",
        [],
    );

    // Lightweight migration: add `provider` to saved_orgs tables created before
    // multi-provider support. Pre-existing rows are Azure DevOps connections.
    let _ = conn.execute(
        "ALTER TABLE saved_orgs ADD COLUMN provider TEXT NOT NULL DEFAULT 'ado'",
        [],
    );

    Ok(conn)
}

fn dirs_db_path() -> Result<String, AppError> {
    let data_dir = dirs_data_dir()?;
    std::fs::create_dir_all(&data_dir)
        .map_err(|_e| AppError::Cache(rusqlite::Error::InvalidPath(data_dir.clone().into())))?;
    Ok(format!("{}/pex.db", data_dir))
}

/// Directory where opt-in review diagnostic traces are written
/// (`<data_dir>/diagnostics`). Created on demand.
pub fn diagnostics_dir() -> Result<String, AppError> {
    let dir = format!("{}/diagnostics", dirs_data_dir()?);
    std::fs::create_dir_all(&dir)
        .map_err(|_e| AppError::Cache(rusqlite::Error::InvalidPath(dir.clone().into())))?;
    Ok(dir)
}

fn dirs_data_dir() -> Result<String, AppError> {
    // Use XDG_DATA_HOME on Linux, AppData on Windows, Application Support on macOS
    if let Ok(dir) = std::env::var("XDG_DATA_HOME") {
        return Ok(format!("{}/pex", dir));
    }
    if let Ok(home) = std::env::var("HOME") {
        #[cfg(target_os = "macos")]
        return Ok(format!(
            "{}/Library/Application Support/com.pex.pr-reviewer",
            home
        ));
        #[cfg(target_os = "linux")]
        return Ok(format!("{}/.local/share/pex", home));
    }
    Err(AppError::Cache(rusqlite::Error::InvalidPath(
        "no home dir".into(),
    )))
}

// ---- Viewed Files ----

pub fn set_viewed(
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
            rusqlite::params![org, project, repo, pr, path],
        )?;
    } else {
        conn.execute(
            "DELETE FROM viewed_files WHERE org_url=?1 AND project_id=?2 AND repo_id=?3 AND pr_id=?4 AND file_path=?5",
            rusqlite::params![org, project, repo, pr, path],
        )?;
    }
    Ok(())
}

pub fn get_viewed(
    conn: &Connection,
    org: &str,
    project: &str,
    repo: &str,
    pr: i64,
) -> Result<Vec<String>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT file_path FROM viewed_files WHERE org_url=?1 AND project_id=?2 AND repo_id=?3 AND pr_id=?4"
    )?;
    let paths = stmt
        .query_map(rusqlite::params![org, project, repo, pr], |row| row.get(0))?
        .collect::<Result<Vec<String>, _>>()?;
    Ok(paths)
}

// ---- Saved Orgs ----

pub fn save_org(
    conn: &Connection,
    org_url: &str,
    name: &str,
    token_type: &str,
    provider: &str,
) -> Result<(), AppError> {
    conn.execute(
        "INSERT OR REPLACE INTO saved_orgs (org_url, name, token_type, provider) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![org_url, name, token_type, provider],
    )?;
    Ok(())
}

/// Returns `(org_url, name, token_type, provider)` for each saved connection.
pub fn list_orgs(conn: &Connection) -> Result<Vec<(String, String, String, String)>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT org_url, name, token_type, provider FROM saved_orgs ORDER BY added_at DESC",
    )?;
    let orgs = stmt
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(orgs)
}

pub fn remove_org(conn: &Connection, org_url: &str) -> Result<(), AppError> {
    conn.execute(
        "DELETE FROM saved_orgs WHERE org_url=?1",
        rusqlite::params![org_url],
    )?;
    Ok(())
}

// ---- Settings ----

pub fn get_setting(conn: &Connection, key: &str) -> Result<Option<String>, AppError> {
    let mut stmt = conn.prepare("SELECT value FROM settings WHERE key=?1")?;
    let result = stmt.query_row(rusqlite::params![key], |row| row.get(0));
    match result {
        Ok(val) => Ok(Some(val)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(AppError::Cache(e)),
    }
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<(), AppError> {
    conn.execute(
        "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
        rusqlite::params![key, value],
    )?;
    Ok(())
}

pub fn delete_setting(conn: &Connection, key: &str) -> Result<(), AppError> {
    conn.execute("DELETE FROM settings WHERE key=?1", rusqlite::params![key])?;
    Ok(())
}
