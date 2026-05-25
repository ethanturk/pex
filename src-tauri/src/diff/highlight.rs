use syntect::highlighting::ThemeSet;
use syntect::html::highlighted_html_for_string;
use syntect::parsing::SyntaxSet;

/// Highlight source code to HTML using syntect.
/// Falls back to plain text if no syntax definition matches.
pub fn highlight_code(code: &str, file_path: &str) -> String {
    let ss = SyntaxSet::load_defaults_newlines();
    let ts = ThemeSet::load_defaults();

    let syntax = ss
        .find_syntax_for_file(file_path)
        .ok()
        .flatten()
        .unwrap_or_else(|| ss.find_syntax_plain_text());

    let theme = &ts.themes["base16-ocean.dark"];

    match highlighted_html_for_string(code, &ss, syntax, theme) {
        Ok(html) => html,
        Err(_) => escape_html_with_lines(code),
    }
}

fn escape_html_with_lines(code: &str) -> String {
    code.lines()
        .map(|line| {
            format!(
                "<span>{}</span>",
                line.replace('&', "&amp;")
                    .replace('<', "&lt;")
                    .replace('>', "&gt;")
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}
