use crate::AppError;

/// Identifier for a user-customizable system prompt.
/// The string value is the stable key used both in the SQLite settings table
/// and over the Tauri command boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptKey {
    ExplainHunkSystem,
    ReviewHunkSystem,
}

impl PromptKey {
    pub const ALL: &'static [PromptKey] = &[PromptKey::ExplainHunkSystem, PromptKey::ReviewHunkSystem];

    pub fn as_str(self) -> &'static str {
        match self {
            PromptKey::ExplainHunkSystem => "explain_hunk_system",
            PromptKey::ReviewHunkSystem => "review_hunk_system",
        }
    }

    pub fn from_str(s: &str) -> Result<Self, AppError> {
        match s {
            "explain_hunk_system" => Ok(PromptKey::ExplainHunkSystem),
            "review_hunk_system" => Ok(PromptKey::ReviewHunkSystem),
            other => Err(AppError::Ai(format!("Unknown prompt key: {}", other))),
        }
    }

    /// The default text shipped with the application. The DB-stored value (if any)
    /// overrides this; resetting a prompt deletes the override and falls back here.
    pub fn default_text(self) -> &'static str {
        match self {
            PromptKey::ExplainHunkSystem => DEFAULT_EXPLAIN_HUNK_SYSTEM,
            PromptKey::ReviewHunkSystem => DEFAULT_REVIEW_HUNK_SYSTEM,
        }
    }

    fn db_key(self) -> String {
        format!("ai_prompt_{}", self.as_str())
    }
}

/// Default prompt for explaining a single diff hunk.
pub const DEFAULT_EXPLAIN_HUNK_SYSTEM: &str = r#"You are a helpful code reviewer. Your task is to explain what a single diff hunk changes in clear, concise English.

For the given hunk:
1. Summarize what changed in 1-2 sentences
2. Explain the likely purpose of the change — why it was made
3. Highlight any potential concerns (bugs, performance issues, consistency problems) visible in this hunk

Focus only on this hunk; do not speculate about other changes in the file. Keep your response to 2-3 short paragraphs. Use markdown for formatting. Do not include greetings or sign-offs."#;

/// Default prompt for reviewing a single diff hunk.
pub const DEFAULT_REVIEW_HUNK_SYSTEM: &str = r#"You are a careful code reviewer analyzing a single diff hunk from a pull request. Your task is to provide a concise, actionable review of just this specific change.

For the given hunk:
1. Summarize what this change does in one sentence
2. Identify any issues: bugs, logic errors, edge cases, race conditions, security concerns
3. Suggest improvements if applicable (naming, structure, performance)
4. Note anything that looks good and why

Be specific — reference exact line numbers from the hunk header. Keep your response focused and brief (3-5 bullet points max). Use markdown. Do not include greetings or sign-offs."#;

/// Resolve a prompt: returns the user override from SQLite if present, otherwise the default.
pub fn resolve_prompt(conn: &rusqlite::Connection, key: PromptKey) -> Result<String, AppError> {
    let stored = crate::cache::get_setting(conn, &key.db_key())?;
    Ok(stored
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| key.default_text().to_string()))
}

/// Persist a user override for a prompt.
pub fn save_prompt(conn: &rusqlite::Connection, key: PromptKey, value: &str) -> Result<(), AppError> {
    crate::cache::set_setting(conn, &key.db_key(), value)
}

/// Remove the user override for a prompt (revert to default).
pub fn reset_prompt(conn: &rusqlite::Connection, key: PromptKey) -> Result<(), AppError> {
    crate::cache::delete_setting(conn, &key.db_key())
}

/// Prompt for generating explain-hunk user message.
pub fn explain_hunk_user(file_path: &str, hunk_header: &str, hunk_text: &str) -> String {
    format!(
        "Please explain this hunk from `{}`:\n\n{}\n{}",
        file_path, hunk_header, hunk_text
    )
}

/// Prompt for generating review-hunk user message. `agents` and `style` are the
/// nearest project-conventions / style-guide files, when found (see ai::standards).
/// Sections for unavailable docs are omitted entirely so the model isn't fed empty
/// headings.
pub fn review_hunk_user(
    file_path: &str,
    hunk_header: &str,
    hunk_text: &str,
    agents: Option<(&str, &str)>,
    style: Option<(&str, &str)>,
) -> String {
    let mut out = String::new();
    if let Some((path, content)) = agents {
        out.push_str(&format!(
            "## Project conventions (AGENTS.md, found at `{}`)\n{}\n\n",
            path, content
        ));
    }
    if let Some((path, content)) = style {
        out.push_str(&format!(
            "## Project style guide (STYLE.md, found at `{}`)\n{}\n\n",
            path, content
        ));
    }
    out.push_str(&format!(
        "## Hunk\nReview this hunk from `{}`:\n\n{}\n{}",
        file_path, hunk_header, hunk_text
    ));
    out
}
