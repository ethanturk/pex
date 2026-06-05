//! The trust feedback loop (Phase 3).
//!
//! Captures reviewer verdicts on findings (accepted / dismissed / edited),
//! remembers dismissed findings so they don't re-surface on the next iteration
//! (suppression memory), and aggregates verdicts into calibration metrics so
//! the noise floor can be tuned with evidence instead of vibes.
//!
//! Findings come from an LLM, so their wording drifts between runs and their
//! line numbers drift between iterations. To match "the same finding" across
//! runs we fingerprint on the file path plus a *normalized* comment (lowercased,
//! digits and punctuation stripped, whitespace collapsed) — robust to line-ref
//! churn and minor rephrasing, specific enough to avoid collapsing distinct
//! issues in the same file.

use crate::AppError;
use libsql::{params, Connection};
use std::collections::HashSet;

/// A reviewer's verdict on a finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Posted as-is — the finding was real and useful.
    Accepted,
    /// Explicitly dismissed — noise. Suppressed on future iterations.
    Dismissed,
    /// Posted after editing — real, but the wording needed work.
    Edited,
}

impl Verdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::Accepted => "accepted",
            Verdict::Dismissed => "dismissed",
            Verdict::Edited => "edited",
        }
    }

    pub fn from_str(s: &str) -> Result<Self, AppError> {
        match s {
            "accepted" => Ok(Verdict::Accepted),
            "dismissed" => Ok(Verdict::Dismissed),
            "edited" => Ok(Verdict::Edited),
            other => Err(AppError::Ai(format!("Unknown verdict: {}", other))),
        }
    }
}

/// Normalize a finding comment so cosmetic differences (line numbers, casing,
/// punctuation, whitespace) don't change its fingerprint.
fn normalize_comment(comment: &str) -> String {
    let mut out = String::with_capacity(comment.len());
    let mut prev_space = false;
    for ch in comment.chars() {
        if ch.is_ascii_alphabetic() {
            out.push(ch.to_ascii_lowercase());
            prev_space = false;
        } else if ch.is_whitespace() || ch.is_ascii_digit() || ch.is_ascii_punctuation() {
            // Collapse any run of non-letters into a single space.
            if !prev_space && !out.is_empty() {
                out.push(' ');
                prev_space = true;
            }
        } else {
            // Non-ASCII letters / symbols: keep as-is, they still carry meaning.
            out.push(ch);
            prev_space = false;
        }
    }
    out.trim().to_string()
}

/// Stable 64-bit FNV-1a hash, hex-encoded. Deterministic across runs and
/// versions (unlike `DefaultHasher`), which matters because these fingerprints
/// are persisted and compared across review runs.
fn fnv1a_hex(s: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in s.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{:016x}", hash)
}

/// Fingerprint a finding by file path + normalized comment.
pub fn fingerprint(file_path: &str, comment: &str) -> String {
    fnv1a_hex(&format!("{}\u{0}{}", file_path, normalize_comment(comment)))
}

/// Record (or update) a reviewer's verdict on a finding. The latest verdict for
/// a given (pr_key, fingerprint) wins.
#[allow(clippy::too_many_arguments)]
pub async fn record_verdict(
    conn: &Connection,
    pr_key: &str,
    fingerprint: &str,
    verdict: Verdict,
    file_path: &str,
    severity: &str,
    tier: &str,
    confidence: u8,
    comment: &str,
    sources: &str,
) -> Result<(), AppError> {
    conn.execute(
        "INSERT OR REPLACE INTO finding_verdicts
            (pr_key, fingerprint, verdict, file_path, severity, tier, confidence, comment, sources, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, datetime('now'))",
        params![
            pr_key,
            fingerprint,
            verdict.as_str(),
            file_path,
            severity,
            tier,
            confidence as i64,
            comment,
            sources
        ],
    )
    .await?;
    Ok(())
}

/// Remove a recorded verdict for a finding. Used when the reviewer undoes a
/// dismissal before taking a final action on the finding.
pub async fn clear_verdict(
    conn: &Connection,
    pr_key: &str,
    fingerprint: &str,
) -> Result<(), AppError> {
    conn.execute(
        "DELETE FROM finding_verdicts WHERE pr_key = ?1 AND fingerprint = ?2",
        params![pr_key, fingerprint],
    )
    .await?;
    Ok(())
}

/// The set of fingerprints the reviewer dismissed for this PR. Used to suppress
/// them on subsequent review runs.
pub async fn dismissed_fingerprints(
    conn: &Connection,
    pr_key: &str,
) -> Result<HashSet<String>, AppError> {
    let mut rows = conn
        .query(
            "SELECT fingerprint FROM finding_verdicts WHERE pr_key = ?1 AND verdict = 'dismissed'",
            params![pr_key],
        )
        .await?;
    let mut out = HashSet::new();
    while let Some(row) = rows.next().await? {
        out.insert(row.get::<String>(0)?);
    }
    Ok(out)
}

// ---- Last reviewed iteration (for incremental review) ----

fn last_iteration_key(pr_key: &str) -> String {
    format!("review_last_iteration:{}", pr_key)
}

pub async fn get_last_reviewed_iteration(conn: &Connection, pr_key: &str) -> Option<i32> {
    crate::cache::get_setting(conn, &last_iteration_key(pr_key))
        .await
        .ok()
        .flatten()
        .and_then(|s| s.parse::<i32>().ok())
}

pub async fn set_last_reviewed_iteration(
    conn: &Connection,
    pr_key: &str,
    iteration: i32,
) -> Result<(), AppError> {
    crate::cache::set_setting(conn, &last_iteration_key(pr_key), &iteration.to_string()).await
}

// ---- Calibration ----

/// Accept/dismiss/edit counts for one bucket (a severity or a tier).
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BucketStats {
    pub label: String,
    pub accepted: u32,
    pub dismissed: u32,
    pub edited: u32,
    /// Accept rate (accepted + edited) / total as a percentage; `None` until
    /// at least one finding in this bucket has been acted on. Populated when
    /// the calibration is finalized.
    pub accept_rate: Option<f64>,
}

impl BucketStats {
    fn finalize(&mut self) {
        let total = self.accepted + self.dismissed + self.edited;
        if total > 0 {
            self.accept_rate =
                Some((self.accepted + self.edited) as f64 / total as f64 * 100.0);
        }
    }
}

#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalibrationStats {
    pub total: u32,
    pub accepted: u32,
    pub dismissed: u32,
    pub edited: u32,
    /// Overall accept rate (accepted + edited) / total, as a percentage.
    pub accept_rate: Option<f64>,
    /// Per-severity buckets, in the order critical → moderate → minor.
    pub by_severity: Vec<BucketStats>,
    /// Per-tier buckets, in the order blocking → should-fix → nit → fyi.
    pub by_tier: Vec<BucketStats>,
    /// Per-specialist buckets (Thorough mode). A finding merged from several
    /// specialists credits each, so these may sum to more than `total`.
    /// Findings with no attribution are bucketed under "unattributed".
    pub by_specialist: Vec<BucketStats>,
}

/// Aggregate every recorded verdict (across all PRs) into calibration metrics.
pub async fn calibration(conn: &Connection) -> Result<CalibrationStats, AppError> {
    let mut result_rows = conn
        .query("SELECT verdict, severity, tier, sources FROM finding_verdicts", ())
        .await?;
    let mut rows: Vec<(String, String, String, String)> = Vec::new();
    while let Some(row) = result_rows.next().await? {
        rows.push((
            row.get::<String>(0)?,
            row.get::<String>(1)?,
            row.get::<String>(2)?,
            row.get::<String>(3)?,
        ));
    }

    let mut stats = CalibrationStats::default();
    let mut sev: std::collections::HashMap<String, BucketStats> = std::collections::HashMap::new();
    let mut tier: std::collections::HashMap<String, BucketStats> = std::collections::HashMap::new();
    let mut spec: std::collections::HashMap<String, BucketStats> = std::collections::HashMap::new();

    let bump = |b: &mut BucketStats, v: &str| match v {
        "accepted" => b.accepted += 1,
        "dismissed" => b.dismissed += 1,
        "edited" => b.edited += 1,
        _ => {}
    };

    for (verdict, severity, tier_name, sources) in rows {
        stats.total += 1;
        match verdict.as_str() {
            "accepted" => stats.accepted += 1,
            "dismissed" => stats.dismissed += 1,
            "edited" => stats.edited += 1,
            _ => {}
        }
        let s = sev.entry(severity.clone()).or_default();
        s.label = severity;
        bump(s, &verdict);
        let t = tier.entry(tier_name.clone()).or_default();
        t.label = tier_name;
        bump(t, &verdict);
        // Credit each contributing specialist; a merged finding credits all of
        // them. No attribution → "unattributed".
        let labels: Vec<String> = sources
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let labels = if labels.is_empty() {
            vec!["unattributed".to_string()]
        } else {
            labels
        };
        for label in labels {
            let b = spec.entry(label.clone()).or_default();
            b.label = label;
            bump(b, &verdict);
        }
    }

    if stats.total > 0 {
        stats.accept_rate =
            Some((stats.accepted + stats.edited) as f64 / stats.total as f64 * 100.0);
    }

    // Emit buckets in a stable, meaningful order; drop empties.
    for name in ["critical", "moderate", "minor"] {
        if let Some(mut b) = sev.remove(name) {
            b.finalize();
            stats.by_severity.push(b);
        }
    }
    for name in ["blocking", "should-fix", "nit", "fyi"] {
        if let Some(mut b) = tier.remove(name) {
            b.finalize();
            stats.by_tier.push(b);
        }
    }
    // Specialists: stable order (worst accept rate first so noisy ones stand
    // out), with "unattributed" pinned last.
    let mut specialists: Vec<BucketStats> = spec.into_values().collect();
    for b in &mut specialists {
        b.finalize();
    }
    specialists.sort_by(|a, b| {
        let pin = |x: &BucketStats| (x.label == "unattributed") as u8;
        pin(a)
            .cmp(&pin(b))
            .then(
                a.accept_rate
                    .unwrap_or(0.0)
                    .partial_cmp(&b.accept_rate.unwrap_or(0.0))
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
            .then(a.label.cmp(&b.label))
    });
    stats.by_specialist = specialists;

    Ok(stats)
}

/// Delete all recorded verdicts (resets both calibration and suppression).
pub async fn clear_all(conn: &Connection) -> Result<(), AppError> {
    conn.execute("DELETE FROM finding_verdicts", ()).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_ignores_line_numbers_case_and_punctuation() {
        let a = normalize_comment("Null deref at line 12: `foo` may be None.");
        let b = normalize_comment("null deref at line 5 - foo may be none");
        assert_eq!(a, b);
    }

    #[test]
    fn fingerprint_is_stable_and_path_specific() {
        let f1 = fingerprint("a.rs", "Possible nil at line 3");
        let f2 = fingerprint("a.rs", "possible nil at line 99!");
        let f3 = fingerprint("b.rs", "Possible nil at line 3");
        assert_eq!(f1, f2, "line/case/punct drift must not change the fingerprint");
        assert_ne!(f1, f3, "different files must fingerprint differently");
        assert_eq!(f1.len(), 16);
    }

    async fn mem_db() -> Connection {
        let db = libsql::Builder::new_local(":memory:")
            .build()
            .await
            .unwrap();
        let conn = db.connect().unwrap();
        conn.execute_batch(
            "CREATE TABLE finding_verdicts (
                pr_key TEXT NOT NULL, fingerprint TEXT NOT NULL, verdict TEXT NOT NULL,
                file_path TEXT NOT NULL DEFAULT '', severity TEXT NOT NULL DEFAULT '',
                tier TEXT NOT NULL DEFAULT '', confidence INTEGER NOT NULL DEFAULT 0,
                comment TEXT NOT NULL DEFAULT '', sources TEXT NOT NULL DEFAULT '',
                updated_at TEXT NOT NULL DEFAULT (datetime('now')),
                PRIMARY KEY (pr_key, fingerprint));
             CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
        )
        .await
        .unwrap();
        // `:memory:` databases are per-connection in libsql, so hand back the
        // same connection the schema was created on rather than a fresh clone.
        conn
    }

    #[tokio::test]
    async fn dismissed_are_remembered_per_pr() {
        let conn = mem_db().await;
        let fp = fingerprint("a.rs", "nit");
        record_verdict(&conn, "pr1", &fp, Verdict::Dismissed, "a.rs", "minor", "nit", 90, "nit", "")
            .await
            .unwrap();
        record_verdict(
            &conn, "pr1", &fingerprint("b.rs", "keep"), Verdict::Accepted, "b.rs", "critical",
            "blocking", 95, "keep", "",
        )
        .await
        .unwrap();

        let dismissed = dismissed_fingerprints(&conn, "pr1").await.unwrap();
        assert!(dismissed.contains(&fp));
        assert_eq!(dismissed.len(), 1, "only dismissed findings are suppressed");
        assert!(dismissed_fingerprints(&conn, "pr2").await.unwrap().is_empty(), "scoped per PR");
    }

    #[tokio::test]
    async fn latest_verdict_wins() {
        let conn = mem_db().await;
        let fp = fingerprint("a.rs", "x");
        record_verdict(&conn, "pr1", &fp, Verdict::Dismissed, "a.rs", "minor", "nit", 80, "x", "").await.unwrap();
        record_verdict(&conn, "pr1", &fp, Verdict::Accepted, "a.rs", "minor", "nit", 80, "x", "").await.unwrap();
        assert!(dismissed_fingerprints(&conn, "pr1").await.unwrap().is_empty());
        assert_eq!(calibration(&conn).await.unwrap().accepted, 1);
    }

    #[tokio::test]
    async fn clear_verdict_removes_dismissal_without_accepting() {
        let conn = mem_db().await;
        let fp = fingerprint("a.rs", "x");
        record_verdict(&conn, "pr1", &fp, Verdict::Dismissed, "a.rs", "minor", "nit", 80, "x", "").await.unwrap();

        clear_verdict(&conn, "pr1", &fp).await.unwrap();

        assert!(dismissed_fingerprints(&conn, "pr1").await.unwrap().is_empty());
        let c = calibration(&conn).await.unwrap();
        assert_eq!(c.total, 0);
        assert_eq!(c.accepted, 0);
        assert_eq!(c.dismissed, 0);
    }

    #[tokio::test]
    async fn calibration_counts_and_rates() {
        let conn = mem_db().await;
        record_verdict(&conn, "p", &fingerprint("a", "null deref"), Verdict::Accepted, "a", "critical", "blocking", 95, "null deref", "code-reviewer").await.unwrap();
        record_verdict(&conn, "p", &fingerprint("a", "rename this"), Verdict::Edited, "a", "moderate", "should-fix", 85, "rename this", "code-reviewer,silent-failure-hunter").await.unwrap();
        record_verdict(&conn, "p", &fingerprint("a", "add a test"), Verdict::Dismissed, "a", "minor", "nit", 80, "add a test", "").await.unwrap();
        let c = calibration(&conn).await.unwrap();
        assert_eq!(c.total, 3);
        assert_eq!(c.accepted, 1);
        assert_eq!(c.edited, 1);
        assert_eq!(c.dismissed, 1);
        // (accepted + edited) / total = 2/3 ≈ 66.7%
        assert!((c.accept_rate.unwrap() - 66.6).abs() < 1.0);
        assert_eq!(c.by_severity.len(), 3);
        assert_eq!(c.by_tier.len(), 3);
        // code-reviewer (2), silent-failure-hunter (1), unattributed (1)
        assert_eq!(c.by_specialist.len(), 3);
        let cr = c.by_specialist.iter().find(|b| b.label == "code-reviewer").unwrap();
        assert_eq!(cr.accepted + cr.edited, 2);
        assert!(c.by_specialist.iter().any(|b| b.label == "unattributed"));
    }

    #[tokio::test]
    async fn last_iteration_round_trips() {
        let conn = mem_db().await;
        assert_eq!(get_last_reviewed_iteration(&conn, "pr1").await, None);
        set_last_reviewed_iteration(&conn, "pr1", 4).await.unwrap();
        assert_eq!(get_last_reviewed_iteration(&conn, "pr1").await, Some(4));
    }
}
