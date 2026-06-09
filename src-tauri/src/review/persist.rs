//! Durable storage of *completed* PR reviews.
//!
//! Distinct from [`crate::review::state`], which persists a single in-progress
//! run for crash/cancel resume. This module stores the finished output (summary
//! + findings) of a review per-PR so it survives app restarts and stays visible
//! until the user is done with it.
//!
//! Lifecycle (`status` column):
//!   - `outstanding` — review finished, not yet handled (shows a PR-list badge).
//!   - `completed`   — user marked it done, or its findings were posted to ADO.
//!
//! Stored rows are deleted outright when the underlying PR is closed/merged.

use crate::review::engine::ReviewOutput;
use crate::AppError;
use serde::Serialize;

pub const STATUS_OUTSTANDING: &str = "outstanding";
pub const STATUS_COMPLETED: &str = "completed";

/// A persisted completed review, as returned to the frontend.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredReview {
    pub pr_key: String,
    pub project_id: String,
    pub repo_id: String,
    pub pr_id: i64,
    pub pr_title: String,
    pub iteration: i32,
    /// `outstanding` | `completed`.
    pub status: String,
    pub output: ReviewOutput,
    pub updated_at: String,
}

/// Insert or update the stored review for `pr_key`. Preserves the original
/// `created_at` on replace; bumps `updated_at` to now.
#[allow(clippy::too_many_arguments)]
pub async fn save_review(
    conn: &libsql::Connection,
    pr_key: &str,
    project_id: &str,
    repo_id: &str,
    pr_id: i64,
    pr_title: &str,
    iteration: i32,
    status: &str,
    output: &ReviewOutput,
) -> Result<(), AppError> {
    let json = serde_json::to_string(output)
        .map_err(|e| AppError::Ai(format!("Failed to serialize review output: {}", e)))?;

    conn.execute(
        "INSERT OR REPLACE INTO pr_reviews
            (pr_key, project_id, repo_id, pr_id, pr_title, iteration, status, output, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8,
            COALESCE((SELECT created_at FROM pr_reviews WHERE pr_key = ?1), datetime('now')),
            datetime('now'))",
        libsql::params![
            pr_key,
            project_id,
            repo_id,
            pr_id,
            pr_title,
            iteration as i64,
            status,
            json
        ],
    )
    .await?;
    Ok(())
}

fn row_to_stored(row: &libsql::Row) -> Result<Option<StoredReview>, AppError> {
    let output_json: String = row.get(7)?;
    // A row whose JSON no longer parses (schema drift in the finding shape) is
    // useless to surface; the caller treats it as absent and can prune it.
    let Ok(output) = serde_json::from_str::<ReviewOutput>(&output_json) else {
        return Ok(None);
    };
    Ok(Some(StoredReview {
        pr_key: row.get(0)?,
        project_id: row.get(1)?,
        repo_id: row.get(2)?,
        pr_id: row.get(3)?,
        pr_title: row.get(4)?,
        iteration: row.get::<i64>(5)? as i32,
        status: row.get(6)?,
        output,
        updated_at: row.get(8)?,
    }))
}

const SELECT_COLS: &str =
    "pr_key, project_id, repo_id, pr_id, pr_title, iteration, status, output, updated_at";

/// Load the stored review for `pr_key`, if any. Prunes the row if its stored
/// JSON fails to parse.
pub async fn get_review(
    conn: &libsql::Connection,
    pr_key: &str,
) -> Result<Option<StoredReview>, AppError> {
    let sql = format!("SELECT {SELECT_COLS} FROM pr_reviews WHERE pr_key = ?1");
    let mut rows = conn.query(&sql, libsql::params![pr_key]).await?;
    match rows.next().await? {
        Some(row) => match row_to_stored(&row)? {
            Some(stored) => Ok(Some(stored)),
            None => {
                let _ = delete_review(conn, pr_key).await;
                Ok(None)
            }
        },
        None => Ok(None),
    }
}

/// All stored reviews, newest first. Rows with unparseable JSON are skipped.
pub async fn list_reviews(conn: &libsql::Connection) -> Result<Vec<StoredReview>, AppError> {
    let sql = format!("SELECT {SELECT_COLS} FROM pr_reviews ORDER BY updated_at DESC");
    let mut rows = conn.query(&sql, ()).await?;
    let mut out = Vec::new();
    while let Some(row) = rows.next().await? {
        if let Some(stored) = row_to_stored(&row)? {
            out.push(stored);
        }
    }
    Ok(out)
}

/// Flip a stored review to `completed` (manual "mark completed" or auto-on-post).
pub async fn mark_completed(conn: &libsql::Connection, pr_key: &str) -> Result<(), AppError> {
    conn.execute(
        "UPDATE pr_reviews SET status = 'completed', updated_at = datetime('now') WHERE pr_key = ?1",
        libsql::params![pr_key],
    )
    .await?;
    Ok(())
}

/// Delete the stored review for `pr_key` (PR closed/merged).
pub async fn delete_review(conn: &libsql::Connection, pr_key: &str) -> Result<(), AppError> {
    conn.execute(
        "DELETE FROM pr_reviews WHERE pr_key = ?1",
        libsql::params![pr_key],
    )
    .await?;
    Ok(())
}

/// How long a stored review is retained before the periodic sweep prunes it.
pub const RETENTION_DAYS: i64 = 14;

/// Delete stored reviews not touched (`updated_at`) within `days`. Catches
/// orphaned rows whose PR never appeared in a refreshed listing — e.g. the PR
/// was deleted, or the user never reopened the repo so the close/merge cleanup
/// in `syncPersistedReviews` never ran. Returns the number of rows removed.
pub async fn delete_reviews_older_than(
    conn: &libsql::Connection,
    days: i64,
) -> Result<u64, AppError> {
    let modifier = format!("-{} days", days);
    let removed = conn
        .execute(
            "DELETE FROM pr_reviews WHERE updated_at < datetime('now', ?1)",
            libsql::params![modifier],
        )
        .await?;
    Ok(removed)
}

/// How often the orphan sweep runs once the app is up.
const CLEANUP_INTERVAL_SECS: u64 = 12 * 60 * 60;
/// Initial delay so the sweep doesn't contend with startup work.
const CLEANUP_STARTUP_DELAY_SECS: u64 = 60;

/// Spawn the periodic retention sweep: deletes stored reviews older than
/// [`RETENTION_DAYS`]. Runs shortly after startup, then every
/// [`CLEANUP_INTERVAL_SECS`]. Safe to spawn unconditionally — a no-op when
/// nothing is stale.
pub fn spawn_review_cleanup(store: crate::db::Store) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(CLEANUP_STARTUP_DELAY_SECS)).await;
        loop {
            let conn = store.conn();
            match delete_reviews_older_than(&conn, RETENTION_DAYS).await {
                Ok(n) if n > 0 => {
                    eprintln!("pr_reviews: pruned {n} review(s) older than {RETENTION_DAYS} days");
                }
                Ok(_) => {}
                Err(e) => eprintln!("pr_reviews: cleanup failed: {e}"),
            }
            tokio::time::sleep(std::time::Duration::from_secs(CLEANUP_INTERVAL_SECS)).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::review::engine::ReviewOutput;
    use libsql::Builder;

    async fn mem() -> libsql::Connection {
        let db = Builder::new_local(":memory:").build().await.unwrap();
        let conn = db.connect().unwrap();
        crate::cache::init_schema(&conn).await.unwrap();
        conn
    }

    fn sample_output() -> ReviewOutput {
        ReviewOutput {
            summary: "looks good".into(),
            findings: vec![],
            health: crate::review::engine::ReviewHealth::Success,
            warnings: 0,
            provider_failures: 0,
            warning_summaries: Vec::new(),
        }
    }

    #[tokio::test]
    async fn save_get_round_trip() {
        let conn = mem().await;
        save_review(
            &conn,
            "org/p/r/7",
            "p",
            "r",
            7,
            "Fix thing",
            3,
            STATUS_OUTSTANDING,
            &sample_output(),
        )
        .await
        .unwrap();

        let got = get_review(&conn, "org/p/r/7").await.unwrap().unwrap();
        assert_eq!(got.pr_id, 7);
        assert_eq!(got.iteration, 3);
        assert_eq!(got.status, STATUS_OUTSTANDING);
        assert_eq!(got.pr_title, "Fix thing");
        assert_eq!(got.output.summary, "looks good");
    }

    #[tokio::test]
    async fn mark_completed_flips_status() {
        let conn = mem().await;
        save_review(
            &conn,
            "k",
            "p",
            "r",
            1,
            "t",
            1,
            STATUS_OUTSTANDING,
            &sample_output(),
        )
        .await
        .unwrap();
        mark_completed(&conn, "k").await.unwrap();
        let got = get_review(&conn, "k").await.unwrap().unwrap();
        assert_eq!(got.status, STATUS_COMPLETED);
    }

    #[tokio::test]
    async fn delete_removes_row() {
        let conn = mem().await;
        save_review(
            &conn,
            "k",
            "p",
            "r",
            1,
            "t",
            1,
            STATUS_OUTSTANDING,
            &sample_output(),
        )
        .await
        .unwrap();
        delete_review(&conn, "k").await.unwrap();
        assert!(get_review(&conn, "k").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn list_returns_saved_rows() {
        let conn = mem().await;
        save_review(
            &conn,
            "a",
            "p",
            "r",
            1,
            "t",
            1,
            STATUS_OUTSTANDING,
            &sample_output(),
        )
        .await
        .unwrap();
        save_review(
            &conn,
            "b",
            "p",
            "r",
            2,
            "t",
            1,
            STATUS_COMPLETED,
            &sample_output(),
        )
        .await
        .unwrap();
        let all = list_reviews(&conn).await.unwrap();
        assert_eq!(all.len(), 2);
    }

    #[tokio::test]
    async fn cleanup_prunes_only_stale_rows() {
        let conn = mem().await;
        // A fresh row (updated_at = now) and a stale one (updated_at 30 days ago).
        save_review(
            &conn,
            "fresh",
            "p",
            "r",
            1,
            "t",
            1,
            STATUS_OUTSTANDING,
            &sample_output(),
        )
        .await
        .unwrap();
        save_review(
            &conn,
            "stale",
            "p",
            "r",
            2,
            "t",
            1,
            STATUS_COMPLETED,
            &sample_output(),
        )
        .await
        .unwrap();
        conn.execute(
            "UPDATE pr_reviews SET updated_at = datetime('now', '-30 days') WHERE pr_key = 'stale'",
            (),
        )
        .await
        .unwrap();

        let removed = delete_reviews_older_than(&conn, RETENTION_DAYS)
            .await
            .unwrap();
        assert_eq!(removed, 1);
        assert!(get_review(&conn, "fresh").await.unwrap().is_some());
        assert!(get_review(&conn, "stale").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn save_preserves_created_at_on_replace() {
        let conn = mem().await;
        save_review(
            &conn,
            "k",
            "p",
            "r",
            1,
            "t",
            1,
            STATUS_OUTSTANDING,
            &sample_output(),
        )
        .await
        .unwrap();
        let created: String = {
            let mut rows = conn
                .query("SELECT created_at FROM pr_reviews WHERE pr_key='k'", ())
                .await
                .unwrap();
            rows.next().await.unwrap().unwrap().get(0).unwrap()
        };
        // Re-save (e.g. a fresh run for the same PR) must not reset created_at.
        save_review(
            &conn,
            "k",
            "p",
            "r",
            1,
            "t",
            2,
            STATUS_OUTSTANDING,
            &sample_output(),
        )
        .await
        .unwrap();
        let created2: String = {
            let mut rows = conn
                .query("SELECT created_at FROM pr_reviews WHERE pr_key='k'", ())
                .await
                .unwrap();
            rows.next().await.unwrap().unwrap().get(0).unwrap()
        };
        assert_eq!(created, created2);
    }
}
