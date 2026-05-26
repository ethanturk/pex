use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::html::{styled_line_to_highlighted_html, IncludeBackground};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

/// Highlight source code to one HTML string per line. Each returned string is
/// already HTML-escaped and wrapped in styled `<span>` tokens, ready to drop
/// into a `.diff-content` slot. Falls back to plain escaped text for lines
/// where syntect fails.
pub fn highlight_lines(code: &str, file_path: &str) -> Vec<String> {
    let ss = SyntaxSet::load_defaults_newlines();
    let ts = ThemeSet::load_defaults();

    let syntax = ss
        .find_syntax_for_file(file_path)
        .ok()
        .flatten()
        .unwrap_or_else(|| ss.find_syntax_plain_text());

    let theme = &ts.themes["base16-ocean.dark"];
    let mut h = HighlightLines::new(syntax, theme);

    LinesWithEndings::from(code)
        .map(|line| match h.highlight_line(line, &ss) {
            Ok(regions) => styled_line_to_highlighted_html(&regions[..], IncludeBackground::No)
                .unwrap_or_else(|_| escape_html(line)),
            Err(_) => escape_html(line),
        })
        .collect()
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
