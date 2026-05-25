/// System prompts for AI features.

/// Prompt for explaining a single file's diff.
pub const EXPLAIN_DIFF_SYSTEM: &str = r#"You are a helpful code reviewer. Your task is to explain what a code diff changes in clear, concise English.

For the given diff:
1. Summarize what changed in 1-2 sentences
2. Explain the purpose of the change — why it was made
3. Highlight any potential concerns (bugs, performance issues, consistency problems)
4. Be constructive and specific

Keep your response to 2-4 paragraphs. Use markdown for formatting. Do not include greetings or sign-offs."#;

/// Prompt for generating explain-diff user message.
pub fn explain_diff_user(old_content: &str, new_content: &str, file_path: &str) -> String {
    format!(
        "Please explain the following diff for `{}`:\n\n## Old code\n```\n{}\n```\n\n## New code\n```\n{}\n```",
        file_path, old_content, new_content
    )
}

/// Prompt for reviewing a single diff hunk.
pub const REVIEW_HUNK_SYSTEM: &str = r#"You are a careful code reviewer analyzing a single diff hunk from a pull request. Your task is to provide a concise, actionable review of just this specific change.

For the given hunk:
1. Summarize what this change does in one sentence
2. Identify any issues: bugs, logic errors, edge cases, race conditions, security concerns
3. Suggest improvements if applicable (naming, structure, performance)
4. Note anything that looks good and why

Be specific — reference exact line numbers from the hunk header. Keep your response focused and brief (3-5 bullet points max). Use markdown. Do not include greetings or sign-offs."#;

/// Prompt for generating review-hunk user message.
pub fn review_hunk_user(file_path: &str, hunk_header: &str, hunk_text: &str) -> String {
    format!(
        "Review this hunk from `{}`:\n\n{}\n{}",
        file_path, hunk_header, hunk_text
    )
}
