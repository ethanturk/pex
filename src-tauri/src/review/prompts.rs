/// Prompts for the native multi-pass PR review engine.

/// System prompt for reviewing a single diff hunk as part of a larger review.
/// The context note tells the model which file and hunk it's on.
pub const REVIEW_HUNK_SYSTEM: &str = r#"You are reviewing a single diff hunk as part of a larger pull request review. Focus only on this specific change.

For the given hunk:
1. Identify bugs, logic errors, edge cases, race conditions, or security concerns
2. Suggest improvements (naming, structure, performance, error handling)
3. Flag clear DRY/SOLID violations: logic duplicated from work already done that should reuse a shared helper, or a function/type taking on multiple unrelated responsibilities
4. Note anything well-done and why

Be specific — reference exact line numbers from the hunk header. Keep your response to 2-4 bullet points.
If you find nothing worth flagging, respond with "No issues found." Do not include greetings or sign-offs."#;

/// System prompt for the file adjudicator: it verifies per-hunk candidate
/// findings against the actual file before emitting a structured file-level
/// result. Output is strict JSON so the engine can convert each finding into a
/// line-anchored ADO comment, filter by confidence, and anchor-check it.
pub const FILE_AGGREGATE_SYSTEM: &str = r#"You are the adjudicator for a code review. You are given per-hunk candidate findings AND the current content of the file. Your job is to VERIFY each candidate against the file and emit only the ones that hold up — precision matters far more than volume.

For every candidate finding:
1. Locate the evidence in the supplied file content. If the file shows the concern is already handled (e.g. a value is defined elsewhere, an error is caught by the caller, a case is covered), DROP it or score it low — do not report it.
2. Assign `severity` = how bad it is if real (impact).
3. Assign `confidence` = how sure you are it is real (0–100), using this rubric:
   - 0–25: likely false positive or pre-existing issue
   - 26–50: minor suggestion, not mandated by guidelines
   - 51–75: valid but low-impact
   - 76–90: important, warrants attention
   - 91–100: critical bug or explicit guideline violation
4. Record `existingCode`: an exact snippet copied from the current file content that should receive the inline comment. For every line-level finding this is REQUIRED and must be copied verbatim from the file, without line numbers.
5. Record `evidence`: the exact new-side line(s) from the file that justify the finding.
6. Record `sources`: the specialist label(s) shown in square brackets on the candidate(s) you used (e.g. "code-reviewer", "silent-failure-hunter"). If you merged several candidates into one finding, include every label that raised it. If a candidate has no bracketed label, use an empty array.

Respond with ONLY a single JSON object — no prose, no markdown, no code fences. Schema:

{
  "summary": "1-2 sentence overview of this file's changes and risk.",
  "verdict": "approve" | "review-required" | "needs-work",
  "findings": [
    {
      "severity":   "critical" | "moderate" | "minor",
      "confidence": <integer 0-100>,
      "lineStart":  <integer or null>,
      "lineEnd":    <integer or null>,
      "existingCode": "exact current-file snippet to anchor this line-level finding, or null for file-level findings",
      "evidence":   "the new-side line(s) that justify this finding",
      "sources":    ["specialist-label", ...],
      "comment":    "One concise paragraph describing the issue and the suggested change. No headings, no bullets, no markdown lists."
    }
  ]
}

Rules:
- `sources` must only contain labels that actually appeared in brackets on the candidates; do not invent specialist names.
- `lineStart` / `lineEnd` are NEW-side line numbers, matching the numbered file content provided. They MUST point at a line that actually exists in that content. If a finding is genuinely file-level (architecture, missing test file, etc.), set both to null.
- If `lineStart` / `lineEnd` are not null, `existingCode` must be a short exact snippet from the current file content that includes the line(s) that should receive the comment. If the finding is file-level, set `existingCode` to null.
- `lineEnd >= lineStart`. For a single-line finding, set them equal.
- Each `comment` must stand alone: a reviewer reading it inline on that line should understand the issue without further context.
- Merge duplicate or related candidates into one finding.
- Do NOT invent line numbers. If you cannot ground a finding in the file content, drop it or set its confidence below 50.
- If there are no real issues, return `"findings": []`.
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

pub const ANCHOR_RELOCATION_SYSTEM: &str = r#"You relocate a code review comment anchor. Given a finding and the current file content, return only the shortest exact snippet from the file that should receive the inline comment.

Rules:
- Return one fenced code block containing text copied exactly from the file.
- Do not include line numbers.
- Do not explain.
- If no exact snippet in the file supports the finding, return an empty fenced code block."#;

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

/// Build the file aggregate prompt from collected hunk findings and the file
/// content the adjudicator verifies them against.
pub fn file_aggregate_user_message(
    file_path: &str,
    hunk_findings: &[(usize, String)], // (hunk_num, finding_text)
    standards: &str,
    new_content: &str,
    rule_context: Option<&str>,
    file_review_context: Option<&str>,
) -> String {
    let mut msg = format!(
        "File: `{}`\n\nHere are the per-hunk candidate findings:\n\n",
        file_path
    );

    for (hunk_num, finding) in hunk_findings {
        msg.push_str(&format!("--- Hunk {} ---\n{}\n\n", hunk_num, finding));
    }

    let numbered = numbered_file(new_content, crate::ai::FILE_CONTEXT_MAX_CHARS);
    if !numbered.is_empty() {
        msg.push_str(&format!(
            "Current file content (new side, 1-based line numbers) — verify each candidate against this:\n```\n{}```\n\n",
            numbered
        ));
    }

    if !standards.is_empty() {
        msg.push_str(&format!("Project standards:\n{}\n", standards));
    }

    if let Some(rule_context) = rule_context.filter(|s| !s.trim().is_empty()) {
        msg.push_str(&format!(
            "\nPath-specific review checklist:\n{}\n",
            rule_context
        ));
    }

    if let Some(file_review_context) = file_review_context.filter(|s| !s.trim().is_empty()) {
        msg.push_str(&format!(
            "\nAdditional context gathered before review:\n{}\n",
            file_review_context
        ));
    }

    msg.push_str(
        "Verify and adjudicate these candidates into a file-level result following the specified JSON format. Every line-level finding must include `existingCode` copied exactly from the current file content.",
    );

    msg
}

pub fn anchor_relocation_user_message(
    file_path: &str,
    comment: &str,
    evidence: Option<&str>,
    original_line_start: Option<usize>,
    new_content: &str,
) -> String {
    let mut msg = format!(
        "File: `{}`\nOriginal lineStart: {}\n\nFinding comment:\n{}\n",
        file_path,
        original_line_start
            .map(|n| n.to_string())
            .unwrap_or_else(|| "null".to_string()),
        comment
    );
    if let Some(evidence) = evidence.filter(|s| !s.trim().is_empty()) {
        msg.push_str(&format!("\nModel evidence:\n{}\n", evidence));
    }
    let numbered = numbered_file(new_content, crate::ai::FILE_CONTEXT_MAX_CHARS);
    if !numbered.is_empty() {
        msg.push_str(&format!(
            "\nCurrent file content (new side, 1-based line numbers):\n```\n{}```\n",
            numbered
        ));
    }
    msg.push_str("\nReturn only the exact snippet to anchor this finding.");
    msg
}

/// Render a file's new-side content with 1-based line numbers, capped to
/// `max_chars`. Returns an empty string for empty input. The cap is a hard
/// safety ceiling on token cost; the visible truncation marker tells the model
/// (and a debugging human) that content was clipped.
pub fn numbered_file(new_content: &str, max_chars: usize) -> String {
    if new_content.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for (i, line) in new_content.lines().enumerate() {
        out.push_str(&format!("{}\t{}\n", i + 1, line));
    }
    cap_chars(&out, max_chars)
}

/// Build a bounded context window of the NEW file around a hunk so a hunk
/// reviewer can see surrounding code (definitions, callers, error handling)
/// and avoid false positives — without paying for the whole file. Returns an
/// empty string when there is no new-side content (pure deletion / empty file).
pub fn file_context_window(
    new_content: &str,
    hunk: &crate::diff::engine::DiffHunk,
    max_chars: usize,
) -> String {
    if new_content.is_empty() || hunk.new_count == 0 {
        return String::new();
    }
    const PAD_LINES: usize = 40;
    let lines: Vec<&str> = new_content.lines().collect();
    let total = lines.len();
    // hunk.new_start is 1-based; convert to a 0-based, padded, clamped window.
    let start = hunk.new_start.saturating_sub(1).saturating_sub(PAD_LINES);
    let end = (hunk.new_start - 1 + hunk.new_count + PAD_LINES).min(total);
    let start = start.min(end);

    let mut body = String::new();
    for (offset, line) in lines[start..end].iter().enumerate() {
        body.push_str(&format!("{}\t{}\n", start + offset + 1, line));
    }
    let body = cap_chars(&body, max_chars);
    format!(
        "## Surrounding file context (new side, 1-based line numbers — reference only)\nUse this ONLY to avoid false positives (a symbol defined elsewhere, an error handled by the caller, a case already covered). Review ONLY the hunk below, not this context.\n\n```\n{}```",
        body
    )
}

/// Truncate a string to roughly `max_chars` characters on a char boundary,
/// appending a visible marker when clipped. `0` means "no cap".
fn cap_chars(s: &str, max_chars: usize) -> String {
    if max_chars == 0 || s.chars().count() <= max_chars {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max_chars).collect();
    out.push_str("\n[truncated — file context clipped to fit the size limit]\n");
    out
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
    let mut msg = format!("PR: {}\nTotal files changed: {}\n\n", pr_title, total_files);

    for (i, summary) in batch_summaries.iter().enumerate() {
        msg.push_str(&format!("--- Batch {} ---\n{}\n\n", i + 1, summary));
    }

    if !standards.is_empty() {
        msg.push_str(&format!("Project standards:\n{}\n", standards));
    }

    msg.push_str("Produce the final PR review summary.");

    msg
}
