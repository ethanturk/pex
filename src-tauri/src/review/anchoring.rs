use crate::diff::engine::DiffHunk;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnchorMatchSource {
    Hunk,
    FullFile,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnchorResolution {
    pub line_start: usize,
    pub line_end: usize,
    pub source: AnchorMatchSource,
    pub normalized: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Candidate {
    line_start: usize,
    line_end: usize,
    normalized: bool,
}

/// Resolve a model-provided exact snippet to a new-side line range.
///
/// Search order is deliberate:
/// 1. reviewed hunk new-side lines, so inline comments stay on changed/context
///    lines the model actually saw;
/// 2. the full new file, so a relocation prompt can still recover from hunk
///    header mistakes or line-number drift.
pub fn resolve_existing_code(
    existing_code: &str,
    new_content: &str,
    hunks: &[DiffHunk],
    original_line_start: Option<usize>,
) -> Option<AnchorResolution> {
    let snippet = clean_snippet(existing_code);
    if snippet.trim().is_empty() {
        return None;
    }
    let snippet_lines = snippet_lines(&snippet);
    if snippet_lines.is_empty() {
        return None;
    }

    let hunk_lines = collect_hunk_lines(hunks);
    if let Some(candidate) = select_candidate(
        find_matches(&hunk_lines, &snippet_lines),
        original_line_start,
    ) {
        return Some(AnchorResolution {
            line_start: candidate.line_start,
            line_end: candidate.line_end,
            source: AnchorMatchSource::Hunk,
            normalized: candidate.normalized,
        });
    }

    let full_lines: Vec<(usize, String)> = new_content
        .lines()
        .enumerate()
        .map(|(idx, line)| (idx + 1, normalize_line_end(line).to_string()))
        .collect();
    select_candidate(
        find_matches(&full_lines, &snippet_lines),
        original_line_start,
    )
    .map(|candidate| AnchorResolution {
        line_start: candidate.line_start,
        line_end: candidate.line_end,
        source: AnchorMatchSource::FullFile,
        normalized: candidate.normalized,
    })
}

pub fn extract_fenced_snippet(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Some(start) = trimmed.find("```") {
        let after_open = &trimmed[start + 3..];
        let after_lang = if let Some(newline) = after_open.find('\n') {
            &after_open[newline + 1..]
        } else {
            after_open
        };
        if let Some(end) = after_lang.find("```") {
            return clean_snippet(&after_lang[..end]);
        }
    }
    clean_snippet(trimmed)
}

fn clean_snippet(snippet: &str) -> String {
    let mut lines: Vec<&str> = snippet.lines().map(normalize_line_end).collect();
    while lines.first().is_some_and(|l| l.trim().is_empty()) {
        lines.remove(0);
    }
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
    lines.join("\n")
}

fn normalize_line_end(line: &str) -> &str {
    line.trim_end_matches(['\r', '\n'])
}

fn snippet_lines(snippet: &str) -> Vec<String> {
    snippet.lines().map(|line| line.to_string()).collect()
}

fn collect_hunk_lines(hunks: &[DiffHunk]) -> Vec<(usize, String)> {
    let mut lines = Vec::new();
    for hunk in hunks {
        for line in &hunk.lines {
            if let Some(new_lineno) = line.new_lineno {
                lines.push((new_lineno, normalize_line_end(&line.content).to_string()));
            }
        }
    }
    lines
}

fn find_matches(content: &[(usize, String)], snippet_lines: &[String]) -> Vec<Candidate> {
    if snippet_lines.is_empty() || content.len() < snippet_lines.len() {
        return Vec::new();
    }
    let mut matches = Vec::new();
    for start in 0..=(content.len() - snippet_lines.len()) {
        let window = &content[start..start + snippet_lines.len()];
        if !is_consecutive(window) {
            continue;
        }
        if window
            .iter()
            .zip(snippet_lines)
            .all(|((_, actual), expected)| actual == expected)
        {
            matches.push(Candidate {
                line_start: window[0].0,
                line_end: window[window.len() - 1].0,
                normalized: false,
            });
            continue;
        }
        if window
            .iter()
            .zip(snippet_lines)
            .all(|((_, actual), expected)| normalize_ws(actual) == normalize_ws(expected))
        {
            matches.push(Candidate {
                line_start: window[0].0,
                line_end: window[window.len() - 1].0,
                normalized: true,
            });
        }
    }
    matches
}

fn is_consecutive(lines: &[(usize, String)]) -> bool {
    lines
        .windows(2)
        .all(|pair| pair[1].0 == pair[0].0.saturating_add(1))
}

fn normalize_ws(line: &str) -> String {
    line.chars().filter(|c| !c.is_whitespace()).collect()
}

fn select_candidate(
    mut candidates: Vec<Candidate>,
    original_line_start: Option<usize>,
) -> Option<Candidate> {
    if candidates.is_empty() {
        return None;
    }
    candidates.sort_by_key(|c| (c.normalized, c.line_start, c.line_end));
    if candidates.len() == 1 {
        return candidates.into_iter().next();
    }
    let original = original_line_start?;
    candidates.into_iter().min_by_key(|c| {
        let distance = if original < c.line_start {
            c.line_start - original
        } else if original > c.line_end {
            original - c.line_end
        } else {
            0
        };
        (distance, c.normalized, c.line_start)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::engine::extract_hunks;

    fn hunks(old: &str, new: &str) -> Vec<DiffHunk> {
        extract_hunks(old, new)
    }

    #[test]
    fn resolves_exact_hunk_match() {
        let old = "a\nb\n";
        let new = "a\nchanged\nb\n";
        let result = resolve_existing_code("changed", new, &hunks(old, new), Some(2)).unwrap();
        assert_eq!(result.line_start, 2);
        assert_eq!(result.line_end, 2);
        assert_eq!(result.source, AnchorMatchSource::Hunk);
        assert!(!result.normalized);
    }

    #[test]
    fn resolves_whitespace_normalized_match() {
        let old = "fn main() {}\n";
        let new = "fn main() {\n    call(value);\n}\n";
        let result =
            resolve_existing_code("call( value );", new, &hunks(old, new), Some(2)).unwrap();
        assert_eq!(result.line_start, 2);
        assert!(result.normalized);
    }

    #[test]
    fn resolves_multi_line_snippet() {
        let old = "a\nb\n";
        let new = "a\nlet x = 1;\nlet y = 2;\nb\n";
        let result =
            resolve_existing_code("let x = 1;\nlet y = 2;", new, &hunks(old, new), None).unwrap();
        assert_eq!((result.line_start, result.line_end), (2, 3));
    }

    #[test]
    fn duplicate_snippet_requires_original_line() {
        let old = "a\n";
        let new = "dup\nmiddle\ndup\n";
        assert!(resolve_existing_code("dup", new, &hunks(old, new), None).is_none());
        let result = resolve_existing_code("dup", new, &hunks(old, new), Some(3)).unwrap();
        assert_eq!(result.line_start, 3);
    }

    #[test]
    fn no_match_returns_none() {
        let old = "a\n";
        let new = "b\n";
        assert!(resolve_existing_code("missing", new, &hunks(old, new), Some(1)).is_none());
    }

    #[test]
    fn falls_back_to_full_file() {
        let old = "top\n1\n2\n3\n4\nold\n";
        let new = "top\n1\n2\n3\n4\nnew\n";
        let result = resolve_existing_code("top", new, &hunks(old, new), Some(1)).unwrap();
        assert_eq!(result.source, AnchorMatchSource::FullFile);
        assert_eq!(result.line_start, 1);
    }

    #[test]
    fn extracts_relocation_fenced_snippet() {
        let raw = "Use:\n```rust\nlet x = 1;\n```\n";
        assert_eq!(extract_fenced_snippet(raw), "let x = 1;");
    }
}
