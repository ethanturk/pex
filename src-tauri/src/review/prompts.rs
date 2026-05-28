/// Prompts for the native multi-pass PR review engine.

/// System prompt for reviewing a single diff hunk as part of a larger review.
/// The context note tells the model which file and hunk it's on.
pub const REVIEW_HUNK_SYSTEM: &str = r#"You are reviewing a single diff hunk as part of a larger pull request review. Focus only on this specific change.

For the given hunk:
1. Identify bugs, logic errors, edge cases, race conditions, or security concerns
2. Suggest improvements (naming, structure, performance, error handling)
3. Note anything well-done and why

Be specific — reference exact line numbers from the hunk header. Keep your response to 2-4 bullet points.
If you find nothing worth flagging, respond with "No issues found." Do not include greetings or sign-offs."#;

/// System prompt for aggregating per-hunk findings into a file-level summary.
/// Output is strict JSON so the engine can convert each finding into a
/// line-anchored ADO comment.
pub const FILE_AGGREGATE_SYSTEM: &str = r#"You synthesize per-hunk code-review findings into a structured file-level result.

Respond with ONLY a single JSON object — no prose, no markdown, no code fences.
The JSON must conform to this schema:

{
  "summary": "1-2 sentence overview of this file's changes and risk.",
  "verdict": "approve" | "review-required" | "needs-work",
  "findings": [
    {
      "severity": "critical" | "moderate" | "minor",
      "lineStart": <integer or null>,
      "lineEnd":   <integer or null>,
      "comment":   "One concise paragraph describing the issue and the suggested change. No headings, no bullets, no markdown lists."
    }
  ]
}

Rules:
- `lineStart` / `lineEnd` refer to NEW-side line numbers from the hunk headers (e.g. "@@ -10,5 +20,8 @@" means the new range starts at 20). If a finding is genuinely file-level (architecture, missing test file, etc.), set both to null.
- `lineEnd >= lineStart`. For a single-line finding, set them equal.
- Each `comment` must stand alone: a reviewer reading it inline on that line should understand the issue without further context.
- Omit findings that duplicate each other; merge related per-hunk findings into one when they describe the same issue.
- If there are no real issues, return `"findings": []`.
- Do not invent line numbers — if you are unsure, set them to null.
"#;

/// System prompt for batching file summaries.
pub const BATCH_AGGREGATE_SYSTEM: &str = r#"You are synthesizing file-level code review summaries into a batch summary covering multiple files.

Given summaries for several files:
1. Identify cross-cutting concerns that span files
2. Group related issues by theme rather than by file
3. Surface patterns: repeated mistakes, inconsistent approaches, architectural concerns
4. Produce a structured summary

Format:

## Batch Summary
**Files covered:** <list>

### Cross-cutting concerns
- ...

### File highlights
| File | Verdict | Key issues |
|------|---------|------------|
| ... | ... | ... |

Do not include greetings or sign-offs."#;

/// System prompt for the final synthesis across all batches.
pub const FINAL_SYNTHESIS_SYSTEM: &str = r#"You are producing the final code review summary for a pull request.

Given batch summaries covering all changed files:
1. Produce an overall verdict: Approve / Approve with Suggestions / Request Changes
2. List the 3-5 most important findings
3. Group remaining findings by theme
4. Be constructive and actionable

Format:

# PR Review Summary

**Overall Verdict:** <verdict>

## Key Findings
1. **<title>** — <description> (files: ...)
2. ...

## By Theme

### <Theme>
- ...

### <Theme>
- ...

Do not include a Statistics section; the app appends exact counts from structured findings.
Do not include greetings or sign-offs."#;

/// Build a user message for a hunk review, including the hunk text and file context.
pub fn hunk_user_message(
    file_path: &str,
    hunk_header: &str,
    hunk_text: &str,
    standards: &str,
) -> String {
    let mut msg = format!("File: `{}`\n\n{}\n{}", file_path, hunk_header, hunk_text);

    if !standards.is_empty() {
        msg.push_str(&format!("\n\nProject standards:\n{}", standards));
    }

    msg
}

/// Build a context note that tells the LLM which hunk it's on (used as a user message
/// between hunk reviews to maintain conversation flow).
pub fn hunk_context_note(file_path: &str, hunk_num: usize, total_hunks: usize) -> String {
    format!(
        "Now reviewing hunk {}/{} in `{}`.",
        hunk_num, total_hunks, file_path
    )
}

/// Build the file aggregate prompt from collected hunk findings.
pub fn file_aggregate_user_message(
    file_path: &str,
    hunk_findings: &[(usize, String)], // (hunk_num, finding_text)
    standards: &str,
) -> String {
    let mut msg = format!(
        "File: `{}`\n\nHere are the per-hunk findings:\n\n",
        file_path
    );

    for (hunk_num, finding) in hunk_findings {
        msg.push_str(&format!("--- Hunk {} ---\n{}\n\n", hunk_num, finding));
    }

    if !standards.is_empty() {
        msg.push_str(&format!("Project standards:\n{}\n", standards));
    }

    msg.push_str("Synthesize these into a file-level summary following the specified format.");

    msg
}

/// Build the batch aggregate prompt from file summaries.
pub fn batch_aggregate_user_message(
    batch_num: usize,
    total_batches: usize,
    file_summaries: &[(String, String)], // (file_path, summary)
    standards: &str,
) -> String {
    let files: Vec<_> = file_summaries.iter().map(|(p, _)| p.as_str()).collect();
    let mut msg = format!(
        "Batch {}/{} — files: {}\n\n",
        batch_num,
        total_batches,
        files.join(", ")
    );

    for (file_path, summary) in file_summaries {
        msg.push_str(&format!("--- {} ---\n{}\n\n", file_path, summary));
    }

    if !standards.is_empty() {
        msg.push_str(&format!("Project standards:\n{}\n", standards));
    }

    msg.push_str("Synthesize these into a batch summary.");

    msg
}

/// Build the final synthesis prompt from batch summaries.
pub fn final_synthesis_user_message(
    pr_title: &str,
    total_files: usize,
    batch_summaries: &[String],
    standards: &str,
) -> String {
    let mut msg = format!(
        "PR: {}\nTotal files changed: {}\n\n",
        pr_title, total_files
    );

    for (i, summary) in batch_summaries.iter().enumerate() {
        msg.push_str(&format!("--- Batch {} ---\n{}\n\n", i + 1, summary));
    }

    if !standards.is_empty() {
        msg.push_str(&format!("Project standards:\n{}\n", standards));
    }

    msg.push_str("Produce the final PR review summary.");

    msg
}
