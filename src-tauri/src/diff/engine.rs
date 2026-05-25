use similar::{ChangeTag, TextDiff};

/// Process a unified diff into an HTML representation with line numbers,
/// add/remove markers, and conflict detection.
pub fn diff_to_html(old: &str, new: &str) -> String {
    let diff = TextDiff::from_lines(old, new);
    let mut html = String::new();

    for group in diff.grouped_ops(3).iter() {
        let (old_start, new_start) = diff_offsets(group.first().unwrap());

        // Context before changes
        for op in group.iter() {
            for change in diff.iter_changes(op) {
                let (sign, class) = match change.tag() {
                    ChangeTag::Delete => ("-", "diff-remove"),
                    ChangeTag::Insert => ("+", "diff-add"),
                    ChangeTag::Equal => (" ", ""),
                };

                let line_num = match change.tag() {
                    ChangeTag::Delete => {
                        old_start + change.old_index().unwrap_or(0) as i64 + 1
                    }
                    ChangeTag::Insert => {
                        new_start + change.new_index().unwrap_or(0) as i64 + 1
                    }
                    ChangeTag::Equal => 0,
                };

                // Conflict detection
                let conflict_class = if change.value().starts_with("<<<<<<<")
                    || change.value().starts_with("=======")
                    || change.value().starts_with(">>>>>>>")
                {
                    " diff-conflict"
                } else {
                    ""
                };

                if change.tag() == ChangeTag::Equal {
                    html.push_str(&format!(
                        r#"<div class="diff-line {class}{conflict_class}" data-line="{line_num}"><span class="diff-lineno">{}</span>{}</div>"#,
                        line_num,
                        escape_html(change.value()),
                    ));
                } else {
                    html.push_str(&format!(
                        r#"<div class="diff-line {class}{conflict_class}" data-line="{line_num}"><span class="diff-lineno">{}</span><span>{sign} </span>{}</div>"#,
                        line_num,
                        escape_html(change.value()),
                    ));
                }
            }
        }
    }

    html
}

fn diff_offsets(op: &similar::DiffOp) -> (i64, i64) {
    use similar::DiffOp;
    match op {
        DiffOp::Equal { old_index, new_index, .. } => (*old_index as i64, *new_index as i64),
        DiffOp::Delete { old_index, new_index, .. } => (*old_index as i64, *new_index as i64),
        DiffOp::Insert { old_index, new_index, .. } => (*old_index as i64, *new_index as i64),
        DiffOp::Replace { old_index, new_index, .. } => (*old_index as i64, *new_index as i64),
    }
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
