use similar::{ChangeTag, TextDiff};

const CONTEXT_LINES: usize = 3;

/// Compute a unified diff between old and new content, with syntax-aware HTML output.
/// Returns HTML with diff markers, line numbers, conflict detection, and syntax highlighting.
/// Between (and around) hunks, emits `<div class="diff-expander">` markers carrying the
/// hidden 1-based line range on both sides, so the frontend can fetch and inject context.
pub fn highlighted_diff(old: &str, new: &str, file_path: &str) -> String {
    let _highlighted_old = super::highlight::highlight_code(old, file_path);
    let _highlighted_new = super::highlight::highlight_code(new, file_path);

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

                let escaped = escape_html(content);

                html.push_str(&format!(
                    r#"<div class="diff-line {css_class}{conflict_class}" data-line="{line_num}"><span class="diff-lineno">{line_num}</span><span class="diff-sign">{sign} </span><span class="diff-content">{escaped}</span></div>"#,
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
        r#"<div class="diff-expander" data-old-start="{old_start}" data-old-end="{old_end}" data-new-start="{new_start}" data-new-end="{new_end}"><button class="diff-expander-btn" data-action="up" title="Show 10 lines above hunk below">↑ 10</button><button class="diff-expander-btn diff-expander-all" data-action="all" title="Show all hidden lines">{label}</button><button class="diff-expander-btn" data-action="down" title="Show 10 lines below hunk above">↓ 10</button></div>"#,
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
    fn test_escape_html() {
        assert_eq!(escape_html("<script>"), "&lt;script&gt;");
        assert_eq!(escape_html("a & b"), "a &amp; b");
    }
}
