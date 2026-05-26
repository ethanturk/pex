use similar::{ChangeTag, TextDiff};

const CONTEXT_LINES: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffView {
    Inline,
    Split,
}

impl DiffView {
    pub fn from_str(s: &str) -> Self {
        match s {
            "split" => DiffView::Split,
            _ => DiffView::Inline,
        }
    }
}

/// Render a diff in the requested view.
pub fn highlighted_diff_view(old: &str, new: &str, file_path: &str, view: DiffView) -> String {
    match view {
        DiffView::Inline => highlighted_diff(old, new, file_path),
        DiffView::Split => split_diff_html(old, new, file_path),
    }
}

/// Compute a unified diff between old and new content, with syntax-aware HTML output.
/// Returns HTML with diff markers, line numbers, conflict detection, and syntax highlighting.
/// Between (and around) hunks, emits `<div class="diff-expander">` markers carrying the
/// hidden 1-based line range on both sides, so the frontend can fetch and inject context.
pub fn highlighted_diff(old: &str, new: &str, file_path: &str) -> String {
    let old_hl = super::highlight::highlight_lines(old, file_path);
    let new_hl = super::highlight::highlight_lines(new, file_path);

    let diff = TextDiff::from_lines(old, new);
    let groups = diff.grouped_ops(CONTEXT_LINES);
    let old_total = old.lines().count();
    let new_total = new.lines().count();

    let mut html = String::from("<div class=\"diff-container font-mono text-[13px] leading-5\">");

    // Track the last 1-based line number emitted on each side. We've shown lines
    // [1..=prev_old] / [1..=prev_new]; anything beyond up to the next hunk start is hidden.
    let mut prev_old: usize = 0;
    let mut prev_new: usize = 0;

    for group in &groups {
        // Hunk extent on each side (1-based inclusive).
        let first = group.first().unwrap();
        let last = group.last().unwrap();
        let hunk_old_start = first.old_range().start + 1;
        let hunk_new_start = first.new_range().start + 1;
        let hunk_old_end = last.old_range().end;
        let hunk_new_end = last.new_range().end;

        // Gap before this hunk: lines (prev+1 .. hunk_start-1) are hidden.
        let gap_old_start = prev_old + 1;
        let gap_old_end = hunk_old_start.saturating_sub(1);
        let gap_new_start = prev_new + 1;
        let gap_new_end = hunk_new_start.saturating_sub(1);
        if gap_old_end >= gap_old_start || gap_new_end >= gap_new_start {
            html.push_str(&expander_html(
                gap_old_start,
                gap_old_end,
                gap_new_start,
                gap_new_end,
            ));
        }

        // Hunk body.
        for op in group {
            for change in diff.iter_changes(op) {
                let (sign, css_class) = match change.tag() {
                    ChangeTag::Delete => ("-", "diff-remove"),
                    ChangeTag::Insert => ("+", "diff-add"),
                    ChangeTag::Equal => ("", ""),
                };

                let line_num = match change.tag() {
                    ChangeTag::Delete => change.old_index().map(|i| i as i64 + 1).unwrap_or(0),
                    ChangeTag::Insert => change.new_index().map(|i| i as i64 + 1).unwrap_or(0),
                    ChangeTag::Equal => change.new_index().map(|i| i as i64 + 1).unwrap_or(0),
                };

                let content = change.value();

                let conflict_class = if content.starts_with("<<<<<<<")
                    || content.starts_with("=======")
                    || content.starts_with(">>>>>>>")
                {
                    " diff-conflict"
                } else {
                    ""
                };

                let rendered = match change.tag() {
                    ChangeTag::Delete => change.old_index().and_then(|i| old_hl.get(i)).cloned(),
                    ChangeTag::Insert | ChangeTag::Equal => {
                        change.new_index().and_then(|i| new_hl.get(i)).cloned()
                    }
                }
                .unwrap_or_else(|| escape_html(content));

                html.push_str(&format!(
                    r#"<div class="diff-line {css_class}{conflict_class}" data-line="{line_num}"><span class="diff-lineno">{line_num}</span><span class="diff-sign">{sign} </span><span class="diff-content">{rendered}</span></div>"#,
                ));
            }
        }

        prev_old = hunk_old_end;
        prev_new = hunk_new_end;
    }

    // Trailing gap after the last hunk.
    let tail_old_start = prev_old + 1;
    let tail_new_start = prev_new + 1;
    if tail_old_start <= old_total || tail_new_start <= new_total {
        html.push_str(&expander_html(
            tail_old_start,
            old_total,
            tail_new_start,
            new_total,
        ));
    }

    html.push_str("</div>");
    html
}

/// Side-by-side renderer. Emits `.diff-row` rows with paired old/new cells.
/// Only the new-side cell carries `data-line`, so the existing selection /
/// comment flow continues to target new-side lines.
pub fn split_diff_html(old: &str, new: &str, file_path: &str) -> String {
    let old_hl = super::highlight::highlight_lines(old, file_path);
    let new_hl = super::highlight::highlight_lines(new, file_path);

    let diff = TextDiff::from_lines(old, new);
    let groups = diff.grouped_ops(CONTEXT_LINES);
    let old_total = old.lines().count();
    let new_total = new.lines().count();

    let mut html =
        String::from("<div class=\"diff-container diff-split font-mono text-[13px] leading-5\">");

    let mut prev_old: usize = 0;
    let mut prev_new: usize = 0;

    for group in &groups {
        let first = group.first().unwrap();
        let last = group.last().unwrap();
        let hunk_old_start = first.old_range().start + 1;
        let hunk_new_start = first.new_range().start + 1;
        let hunk_old_end = last.old_range().end;
        let hunk_new_end = last.new_range().end;

        let gap_old_start = prev_old + 1;
        let gap_old_end = hunk_old_start.saturating_sub(1);
        let gap_new_start = prev_new + 1;
        let gap_new_end = hunk_new_start.saturating_sub(1);
        if gap_old_end >= gap_old_start || gap_new_end >= gap_new_start {
            html.push_str(&expander_html_split(
                gap_old_start,
                gap_old_end,
                gap_new_start,
                gap_new_end,
            ));
        }

        // Collect deletes + inserts + equals as a linear stream of (line, raw, rendered),
        // then pair contiguous del/ins runs to render side-by-side rows.
        // `raw` is needed for conflict-marker detection; `rendered` is the highlighted HTML.
        let mut pending_del: Vec<(usize, String, String)> = Vec::new();
        let mut pending_ins: Vec<(usize, String, String)> = Vec::new();

        let flush = |html: &mut String,
                     dels: &mut Vec<(usize, String, String)>,
                     inss: &mut Vec<(usize, String, String)>| {
            let n = dels.len().max(inss.len());
            for i in 0..n {
                let old_cell = dels
                    .get(i)
                    .map(|(ln, raw, rendered)| {
                        render_cell(Some(*ln), "-", raw, rendered, "diff-remove", false)
                    })
                    .unwrap_or_else(empty_cell);
                let new_cell = inss
                    .get(i)
                    .map(|(ln, raw, rendered)| {
                        render_cell(Some(*ln), "+", raw, rendered, "diff-add", true)
                    })
                    .unwrap_or_else(empty_cell);
                html.push_str(&format!(
                    "<div class=\"diff-row\">{old_cell}{new_cell}</div>"
                ));
            }
            dels.clear();
            inss.clear();
        };

        for op in group {
            for change in diff.iter_changes(op) {
                let content = change.value().to_string();
                match change.tag() {
                    ChangeTag::Delete => {
                        let n = change.old_index().map(|i| i + 1).unwrap_or(0);
                        let rendered = change
                            .old_index()
                            .and_then(|i| old_hl.get(i))
                            .cloned()
                            .unwrap_or_else(|| escape_html(&content));
                        pending_del.push((n, content, rendered));
                    }
                    ChangeTag::Insert => {
                        let n = change.new_index().map(|i| i + 1).unwrap_or(0);
                        let rendered = change
                            .new_index()
                            .and_then(|i| new_hl.get(i))
                            .cloned()
                            .unwrap_or_else(|| escape_html(&content));
                        pending_ins.push((n, content, rendered));
                    }
                    ChangeTag::Equal => {
                        flush(&mut html, &mut pending_del, &mut pending_ins);
                        let old_n = change.old_index().map(|i| i + 1).unwrap_or(0);
                        let new_n = change.new_index().map(|i| i + 1).unwrap_or(0);
                        let old_rendered = change
                            .old_index()
                            .and_then(|i| old_hl.get(i))
                            .cloned()
                            .unwrap_or_else(|| escape_html(&content));
                        let new_rendered = change
                            .new_index()
                            .and_then(|i| new_hl.get(i))
                            .cloned()
                            .unwrap_or_else(|| escape_html(&content));
                        let old_cell =
                            render_cell(Some(old_n), " ", &content, &old_rendered, "", false);
                        let new_cell =
                            render_cell(Some(new_n), " ", &content, &new_rendered, "", true);
                        html.push_str(&format!(
                            "<div class=\"diff-row\">{old_cell}{new_cell}</div>"
                        ));
                    }
                }
            }
        }
        flush(&mut html, &mut pending_del, &mut pending_ins);

        prev_old = hunk_old_end;
        prev_new = hunk_new_end;
    }

    let tail_old_start = prev_old + 1;
    let tail_new_start = prev_new + 1;
    if tail_old_start <= old_total || tail_new_start <= new_total {
        html.push_str(&expander_html_split(
            tail_old_start,
            old_total,
            tail_new_start,
            new_total,
        ));
    }

    html.push_str("</div>");
    html
}

fn render_cell(
    line_num: Option<usize>,
    sign: &str,
    raw: &str,
    rendered: &str,
    css_class: &str,
    is_new_side: bool,
) -> String {
    let conflict_class = if raw.starts_with("<<<<<<<")
        || raw.starts_with("=======")
        || raw.starts_with(">>>>>>>")
    {
        " diff-conflict"
    } else {
        ""
    };
    let ln_text = line_num.map(|n| n.to_string()).unwrap_or_default();
    let data_line_attr = if is_new_side {
        format!(" data-line=\"{}\"", line_num.unwrap_or(0))
    } else {
        String::new()
    };
    format!(
        r#"<div class="diff-cell diff-line {css_class}{conflict_class}"{data_line_attr}><span class="diff-lineno">{ln_text}</span><span class="diff-sign">{sign} </span><span class="diff-content">{rendered}</span></div>"#,
    )
}

fn empty_cell() -> String {
    r#"<div class="diff-cell diff-cell--empty"></div>"#.to_string()
}

fn expander_html_split(
    old_start: usize,
    old_end: usize,
    new_start: usize,
    new_end: usize,
) -> String {
    let new_count = new_end.saturating_sub(new_start.saturating_sub(1));
    let old_count = old_end.saturating_sub(old_start.saturating_sub(1));
    let count = new_count.max(old_count);
    if count == 0 {
        return String::new();
    }
    let label = if count == 1 {
        "1 hidden line".to_string()
    } else {
        format!("{} hidden lines", count)
    };
    format!(
        r#"<div class="diff-expander diff-expander--split" data-old-start="{old_start}" data-old-end="{old_end}" data-new-start="{new_start}" data-new-end="{new_end}"><button class="diff-expander-btn" data-action="up" title="Show 10 more lines from the top of this gap">↑ 10</button><button class="diff-expander-btn diff-expander-all" data-action="all" title="Show all hidden lines">{label}</button><button class="diff-expander-btn" data-action="down" title="Show 10 more lines from the bottom of this gap">↓ 10</button></div>"#,
    )
}

/// Render an expander control. `old_*` / `new_*` are 1-based inclusive ranges of hidden lines.
/// When a side has zero hidden lines (e.g. file added → no old side), pass start > end.
fn expander_html(
    old_start: usize,
    old_end: usize,
    new_start: usize,
    new_end: usize,
) -> String {
    let new_count = new_end.saturating_sub(new_start.saturating_sub(1));
    let old_count = old_end.saturating_sub(old_start.saturating_sub(1));
    let count = new_count.max(old_count);
    if count == 0 {
        return String::new();
    }
    let label = if count == 1 {
        "1 hidden line".to_string()
    } else {
        format!("{} hidden lines", count)
    };
    format!(
        r#"<div class="diff-expander" data-old-start="{old_start}" data-old-end="{old_end}" data-new-start="{new_start}" data-new-end="{new_end}"><button class="diff-expander-btn" data-action="up" title="Show 10 more lines from the top of this gap">↑ 10</button><button class="diff-expander-btn diff-expander-all" data-action="all" title="Show all hidden lines">{label}</button><button class="diff-expander-btn" data-action="down" title="Show 10 more lines from the bottom of this gap">↓ 10</button></div>"#,
    )
}

pub fn diff_to_html(old: &str, new: &str) -> String {
    highlighted_diff(old, new, ".txt")
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

// ---- Hunk extraction ----

/// A structured diff hunk for per-hunk AI review.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DiffHunk {
    /// 0-based hunk index
    pub index: usize,
    /// Unified diff header like "@@ -1,4 +1,5 @@"
    pub header: String,
    /// 1-based start line in old file
    pub old_start: usize,
    /// Number of lines in old file for this hunk
    pub old_count: usize,
    /// 1-based start line in new file
    pub new_start: usize,
    /// Number of lines in new file for this hunk
    pub new_count: usize,
    /// Individual lines in the hunk
    pub lines: Vec<HunkLine>,
}

/// A single line within a diff hunk.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HunkLine {
    /// "+"  for insert, "-" for delete, " " for context
    pub kind: String,
    /// Line number in the NEW file (None for deleted lines)
    pub new_lineno: Option<usize>,
    /// Line number in the OLD file (None for inserted lines)
    pub old_lineno: Option<usize>,
    /// The line content (without leading +/-/space)
    pub content: String,
}

/// Extract structured hunks from a diff between old and new content.
pub fn extract_hunks(old: &str, new: &str) -> Vec<DiffHunk> {
    let diff = similar::TextDiff::from_lines(old, new);
    let groups = diff.grouped_ops(CONTEXT_LINES);

    groups
        .iter()
        .enumerate()
        .map(|(hunk_idx, group)| {
            let first = group.first().unwrap();
            let last = group.last().unwrap();

            let old_start = first.old_range().start + 1;
            let old_end = last.old_range().end;
            let new_start = first.new_range().start + 1;
            let new_end = last.new_range().end;

            let old_count = old_end.saturating_sub(old_start.saturating_sub(1));
            let new_count = new_end.saturating_sub(new_start.saturating_sub(1));

            let header = if old_count == 0 {
                format!("@@ -0,0 +{},{} @@", new_start, new_count)
            } else if new_count == 0 {
                format!("@@ -{},{} +0,0 @@", old_start, old_count)
            } else {
                format!(
                    "@@ -{},{} +{},{} @@",
                    old_start, old_count, new_start, new_count
                )
            };

            let lines: Vec<HunkLine> = group
                .iter()
                .flat_map(|op| diff.iter_changes(op))
                .map(|change| {
                    let (kind, old_lineno, new_lineno) = match change.tag() {
                        similar::ChangeTag::Delete => (
                            "-".to_string(),
                            change.old_index().map(|i| i + 1),
                            None,
                        ),
                        similar::ChangeTag::Insert => (
                            "+".to_string(),
                            None,
                            change.new_index().map(|i| i + 1),
                        ),
                        similar::ChangeTag::Equal => (
                            " ".to_string(),
                            change.old_index().map(|i| i + 1),
                            change.new_index().map(|i| i + 1),
                        ),
                    };

                    HunkLine {
                        kind,
                        new_lineno,
                        old_lineno,
                        content: change.value().to_string(),
                    }
                })
                .collect();

            DiffHunk {
                index: hunk_idx,
                header,
                old_start,
                old_count,
                new_start,
                new_count,
                lines,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diff_added_line() {
        let old = "line1\n";
        let new = "line1\nline2\n";
        let html = highlighted_diff(old, new, ".txt");
        assert!(html.contains("diff-add"));
        assert!(html.contains("line2"));
    }

    #[test]
    fn test_diff_new_file_realistic_python() {
        // Mirror the kind of content the user is fetching successfully (4645
        // bytes of Python) — triple-quoted docstring, special chars, etc.
        let old = "";
        let new = r#""""EmbeddingReliabilityEvaluator — the v1 PO-mandated reliability gate.

Distinguishes embedding *freshness* (a reliability check, not a boost) from
document *freshness* (a recency boost handled elsewhere).
"""
import dataclasses
from typing import Optional, List

class EmbeddingReliabilityEvaluator:
    def __init__(self, threshold: float = 0.7) -> None:
        self.threshold = threshold

    def evaluate(self, score: float) -> bool:
        return score >= self.threshold
"#;
        let html = highlighted_diff(old, new, "embedding_reliability_evaluator.py");
        eprintln!("PY-INSERT HTML LEN: {}", html.len());
        eprintln!("PY-INSERT HTML (first 600 chars):\n{}", &html.chars().take(600).collect::<String>());
        assert!(html.contains("EmbeddingReliabilityEvaluator"), "should contain class name");
        assert!(html.contains("diff-add"), "should mark inserts");
        assert!(html.len() > 500, "html should be substantial, got {} bytes", html.len());
    }

    #[test]
    fn test_diff_new_file_all_insert() {
        // Reproduces the "added file" case where the base side is empty.
        let old = "";
        let new = "line1\nline2\nline3\n";
        let html = highlighted_diff(old, new, ".py");
        eprintln!("ALL-INSERT HTML: {html}");
        assert!(
            html.contains("line1") && html.contains("line2") && html.contains("line3"),
            "rendered HTML should contain every inserted line; got: {html}"
        );
        assert!(
            html.contains("diff-add"),
            "rendered HTML should mark inserted lines with diff-add; got: {html}"
        );
    }

    #[test]
    fn test_diff_removed_line() {
        let old = "line1\nline2\n";
        let new = "line1\n";
        let html = highlighted_diff(old, new, ".txt");
        assert!(html.contains("diff-remove"));
    }

    #[test]
    fn test_diff_no_changes() {
        let old = "line1\nline2\n";
        let new = "line1\nline2\n";
        let html = highlighted_diff(old, new, ".txt");
        assert!(!html.contains("diff-add"));
        assert!(!html.contains("diff-remove"));
    }

    #[test]
    fn test_conflict_detection() {
        let old = "before\n";
        let new = "before\n<<<<<<< ours\nmiddle\n=======\ntheirs\n>>>>>>> theirs\n";
        let html = highlighted_diff(old, new, ".txt");
        assert!(html.contains("diff-conflict"));
    }

    #[test]
    fn test_split_diff_pairs_insert_delete() {
        let old = "alpha\nbeta\n";
        let new = "alpha\nBETA\n";
        let html = split_diff_html(old, new, ".txt");
        assert!(html.contains("diff-split"), "container has diff-split class");
        assert!(html.contains("diff-row"), "emits diff-row wrappers");
        assert!(html.contains("diff-remove") && html.contains("beta"));
        assert!(html.contains("diff-add") && html.contains("BETA"));
        // The new-side cell should carry data-line.
        assert!(html.contains("data-line=\"2\""));
    }

    #[test]
    fn test_split_diff_pure_insert_has_empty_old_cell() {
        let old = "line1\n";
        let new = "line1\nline2\n";
        let html = split_diff_html(old, new, ".txt");
        assert!(html.contains("diff-cell--empty"), "added line gets empty old cell");
        assert!(html.contains("diff-add"));
    }

    #[test]
    fn test_escape_html() {
        assert_eq!(escape_html("<script>"), "&lt;script&gt;");
        assert_eq!(escape_html("a & b"), "a &amp; b");
    }
}
