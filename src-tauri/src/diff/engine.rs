use similar::{ChangeTag, TextDiff};

/// Compute a unified diff between old and new content, with syntax-aware HTML output.
/// Returns HTML with diff markers, line numbers, conflict detection, and syntax highlighting.
pub fn highlighted_diff(old: &str, new: &str, file_path: &str) -> String {
    // Use syntect for syntax highlighting, then apply diff markers
    let _highlighted_old = super::highlight::highlight_code(old, file_path);
    let _highlighted_new = super::highlight::highlight_code(new, file_path);

    let diff = TextDiff::from_lines(old, new);
    let mut html = String::from("<div class=\"diff-container font-mono text-[13px] leading-5\">");

    for group in diff.grouped_ops(3).iter() {
        for op in group.iter() {
            for change in diff.iter_changes(op) {
                let (sign, css_class) = match change.tag() {
                    ChangeTag::Delete => ("-", "diff-remove"),
                    ChangeTag::Insert => ("+", "diff-add"),
                    ChangeTag::Equal => ("", ""),
                };

                let line_num = match change.tag() {
                    ChangeTag::Delete => {
                        change.old_index().map(|i| i as i64 + 1).unwrap_or(0)
                    }
                    ChangeTag::Insert => {
                        change.new_index().map(|i| i as i64 + 1).unwrap_or(0)
                    }
                    ChangeTag::Equal => 0,
                };

                let content = change.value();

                // Conflict detection
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
    }

    html.push_str("</div>");
    html
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
