use crate::AppError;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptModelOverride {
    pub provider_id: Option<String>,
    pub model: String,
}

/// Identifier for a user-customizable system prompt.
/// The string value is the stable key used both in the SQLite settings table
/// and over the Tauri command boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptKey {
    ExplainHunkSystem,
    // Multi-pass specialist prompts (used by Thorough review mode).
    // Modeled after https://github.com/anthropics/claude-code/tree/main/plugins/pr-review-toolkit
    ReviewCodeReviewerSystem,
    ReviewSilentFailureSystem,
    ReviewCommentAnalyzerSystem,
    ReviewTestAnalyzerSystem,
    ReviewTypeDesignSystem,
    ReviewCodeSimplifierSystem,
    ReviewDesignPrinciplesSystem,
}

impl PromptKey {
    pub const ALL: &'static [PromptKey] = &[
        PromptKey::ExplainHunkSystem,
        PromptKey::ReviewCodeReviewerSystem,
        PromptKey::ReviewSilentFailureSystem,
        PromptKey::ReviewCommentAnalyzerSystem,
        PromptKey::ReviewTestAnalyzerSystem,
        PromptKey::ReviewTypeDesignSystem,
        PromptKey::ReviewCodeSimplifierSystem,
        PromptKey::ReviewDesignPrinciplesSystem,
    ];

    /// Specialist prompts used by Thorough multi-pass review. Order here is the
    /// order specialists are run per hunk. This is the full roster the
    /// confirmation dialog offers as toggles; `DEFAULT_THOROUGH_SPECIALISTS`
    /// picks which are enabled by default.
    pub const THOROUGH_SPECIALISTS: &'static [PromptKey] = &[
        PromptKey::ReviewCodeReviewerSystem,
        PromptKey::ReviewSilentFailureSystem,
        PromptKey::ReviewCommentAnalyzerSystem,
        PromptKey::ReviewTestAnalyzerSystem,
        PromptKey::ReviewTypeDesignSystem,
        PromptKey::ReviewDesignPrinciplesSystem,
        PromptKey::ReviewCodeSimplifierSystem,
    ];

    /// The specialists enabled by default in the review confirmation dialog.
    /// A deliberately lean set of three high-signal lenses — correctness/style,
    /// error-handling bugs, and structural design — to keep per-hunk token cost
    /// (and latency on local models) down. The other specialists in
    /// `THOROUGH_SPECIALISTS` remain available as toggles for a deeper pass.
    pub const DEFAULT_THOROUGH_SPECIALISTS: &'static [PromptKey] = &[
        PromptKey::ReviewCodeReviewerSystem,
        PromptKey::ReviewSilentFailureSystem,
        PromptKey::ReviewDesignPrinciplesSystem,
    ];

    /// Whether this specialist is enabled by default (see
    /// `DEFAULT_THOROUGH_SPECIALISTS`).
    pub fn is_default_specialist(self) -> bool {
        Self::DEFAULT_THOROUGH_SPECIALISTS.contains(&self)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            PromptKey::ExplainHunkSystem => "explain_hunk_system",
            PromptKey::ReviewCodeReviewerSystem => "review_code_reviewer_system",
            PromptKey::ReviewSilentFailureSystem => "review_silent_failure_system",
            PromptKey::ReviewCommentAnalyzerSystem => "review_comment_analyzer_system",
            PromptKey::ReviewTestAnalyzerSystem => "review_test_analyzer_system",
            PromptKey::ReviewTypeDesignSystem => "review_type_design_system",
            PromptKey::ReviewCodeSimplifierSystem => "review_code_simplifier_system",
            PromptKey::ReviewDesignPrinciplesSystem => "review_design_principles_system",
        }
    }

    /// Short human-readable name used as a tag on multi-pass findings.
    pub fn specialist_label(self) -> &'static str {
        match self {
            PromptKey::ReviewCodeReviewerSystem => "code-reviewer",
            PromptKey::ReviewSilentFailureSystem => "silent-failure-hunter",
            PromptKey::ReviewCommentAnalyzerSystem => "comment-analyzer",
            PromptKey::ReviewTestAnalyzerSystem => "test-analyzer",
            PromptKey::ReviewTypeDesignSystem => "type-design-analyzer",
            PromptKey::ReviewCodeSimplifierSystem => "code-simplifier",
            PromptKey::ReviewDesignPrinciplesSystem => "design-principles",
            _ => "reviewer",
        }
    }

    pub fn from_str(s: &str) -> Result<Self, AppError> {
        match s {
            "explain_hunk_system" => Ok(PromptKey::ExplainHunkSystem),
            "review_code_reviewer_system" => Ok(PromptKey::ReviewCodeReviewerSystem),
            "review_silent_failure_system" => Ok(PromptKey::ReviewSilentFailureSystem),
            "review_comment_analyzer_system" => Ok(PromptKey::ReviewCommentAnalyzerSystem),
            "review_test_analyzer_system" => Ok(PromptKey::ReviewTestAnalyzerSystem),
            "review_type_design_system" => Ok(PromptKey::ReviewTypeDesignSystem),
            "review_code_simplifier_system" => Ok(PromptKey::ReviewCodeSimplifierSystem),
            "review_design_principles_system" => Ok(PromptKey::ReviewDesignPrinciplesSystem),
            other => Err(AppError::Ai(format!("Unknown prompt key: {}", other))),
        }
    }

    /// The default text shipped with the application. The DB-stored value (if any)
    /// overrides this; resetting a prompt deletes the override and falls back here.
    pub fn default_text(self) -> &'static str {
        match self {
            PromptKey::ExplainHunkSystem => DEFAULT_EXPLAIN_HUNK_SYSTEM,
            PromptKey::ReviewCodeReviewerSystem => DEFAULT_REVIEW_CODE_REVIEWER_SYSTEM,
            PromptKey::ReviewSilentFailureSystem => DEFAULT_REVIEW_SILENT_FAILURE_SYSTEM,
            PromptKey::ReviewCommentAnalyzerSystem => DEFAULT_REVIEW_COMMENT_ANALYZER_SYSTEM,
            PromptKey::ReviewTestAnalyzerSystem => DEFAULT_REVIEW_TEST_ANALYZER_SYSTEM,
            PromptKey::ReviewTypeDesignSystem => DEFAULT_REVIEW_TYPE_DESIGN_SYSTEM,
            PromptKey::ReviewCodeSimplifierSystem => DEFAULT_REVIEW_CODE_SIMPLIFIER_SYSTEM,
            PromptKey::ReviewDesignPrinciplesSystem => DEFAULT_REVIEW_DESIGN_PRINCIPLES_SYSTEM,
        }
    }

    fn db_key(self) -> String {
        format!("ai_prompt_{}", self.as_str())
    }

    fn model_db_key(self) -> String {
        format!("ai_prompt_model_{}", self.as_str())
    }

    fn provider_db_key(self) -> String {
        format!("ai_prompt_provider_{}", self.as_str())
    }
}

/// Default prompt for explaining a single diff hunk.
pub const DEFAULT_EXPLAIN_HUNK_SYSTEM: &str = r#"You are a helpful code reviewer. Your task is to explain what a single diff hunk changes in clear, concise English.

For the given hunk:
1. Summarize what changed in 1-2 sentences
2. Explain the likely purpose of the change — why it was made
3. Highlight any potential concerns (bugs, performance issues, consistency problems) visible in this hunk

Focus only on this hunk; do not speculate about other changes in the file. Keep your response to 2-3 short paragraphs. Use markdown for formatting. Do not include greetings or sign-offs."#;

// ---- Multi-pass specialist prompts ----
//
// Each specialist reviews the SAME hunk through a narrow lens. They share a
// common output contract (bullet points, "No issues found." sentinel) so the
// downstream file-aggregate step can consume them uniformly. Distilled from
// anthropics/claude-code/plugins/pr-review-toolkit.

/// Defaults for the "code-reviewer" specialist — adherence to guidelines, style, best practices.
pub const DEFAULT_REVIEW_CODE_REVIEWER_SYSTEM: &str = r#"You are an elite code reviewer focused on adherence to project guidelines, style, and best practices. You review a single diff hunk as one pass of a multi-agent review.

Flag only what you would block a pull request on, or what an explicit project rule forbids:
1. Violations of the project standards provided to you — style, imports, naming, framework conventions, error handling. When no standards are given, judge against the idiomatic conventions of the language and the surrounding code; do not invent rules.
2. Patterns that contradict established practice visible in this hunk or its surrounding context.
3. Maintainability concerns with real cost: misleading names, needlessly complex logic, or code that will rot.

Only the added/changed (`+`) lines are in scope. Treat context lines (no `+`) as read-only, and assume symbols you cannot see here are defined correctly elsewhere — do not flag them.

Reference exact NEW-side line numbers from the hunk header. Skip nitpicks and subjective rewrites — precision over volume. Keep your response to 2-4 bullet points.

If you find nothing worth flagging, respond with exactly: No issues found.

Do not include greetings or sign-offs."#;

/// Defaults for the "silent-failure-hunter" specialist — error handling and silent failures.
pub const DEFAULT_REVIEW_SILENT_FAILURE_SYSTEM: &str = r#"You are an elite error-handling auditor with zero tolerance for silent failures. You review a single diff hunk as one pass of a multi-agent review.

Scrutinize the hunk for:
1. Empty catch blocks, catch-and-continue, or broad exception swallowing
2. Returning null/undefined/default values on error without logging
3. Optional chaining or null coalescing that silently skips operations
4. Fallback chains and retries that hide problems instead of surfacing them
5. Missing or low-context error logs (no operation name, no IDs, no actionable info)
6. Mock or fake implementations leaking into non-test code

Reference exact NEW-side line numbers from the hunk header. Only flag issues you are confident about. Keep your response to 2-4 bullet points.

If you find nothing worth flagging, respond with exactly: No issues found.

Do not include greetings or sign-offs."#;

/// Defaults for the "comment-analyzer" specialist — comment accuracy and long-term maintainability.
pub const DEFAULT_REVIEW_COMMENT_ANALYZER_SYSTEM: &str = r#"You are an expert at evaluating code comments for accuracy, completeness, and long-term maintainability. You review a single diff hunk as one pass of a multi-agent review.

Scrutinize comments added or modified in this hunk for:
1. Inaccuracy — comment says something the code does not actually do
2. Comments referencing temporary state, transitional implementations, the current task, or specific callers (these rot)
3. Comments that restate WHAT the code does instead of explaining WHY
4. Missing comments where a non-obvious invariant, workaround, or constraint deserves one
5. Stale TODOs / FIXMEs without owner or context

Reference exact NEW-side line numbers from the hunk header. Ignore well-named code with no comments — that is not a defect. Keep your response to 2-4 bullet points.

If you find nothing worth flagging, respond with exactly: No issues found.

Do not include greetings or sign-offs."#;

/// Defaults for the "pr-test-analyzer" specialist — test coverage quality and gaps.
pub const DEFAULT_REVIEW_TEST_ANALYZER_SYSTEM: &str = r#"You are an expert test-coverage analyst. You review a single diff hunk as one pass of a multi-agent review.

For the given hunk, focus on BEHAVIORAL coverage (not line coverage):
1. If this hunk introduces or changes production logic, what critical paths, edge cases, error conditions, or boundary conditions need tests?
2. If this hunk is a test, does it actually exercise behavior — or is it tightly coupled to implementation details that will break on refactor?
3. Missing negative cases, async/concurrent behavior, or integration points
4. Tests that pass without truly asserting the behavior of interest

Reference exact NEW-side line numbers from the hunk header. Be pragmatic — do not demand 100% coverage. Keep your response to 2-4 bullet points.

If you find nothing worth flagging, respond with exactly: No issues found.

Do not include greetings or sign-offs."#;

/// Defaults for the "type-design-analyzer" specialist — type design quality.
pub const DEFAULT_REVIEW_TYPE_DESIGN_SYSTEM: &str = r#"You are an expert at type design and API ergonomics. You review a single diff hunk as one pass of a multi-agent review.

For any type, struct, interface, enum, or schema introduced or modified in this hunk, evaluate:
1. Encapsulation — does the type hide its representation, or does it leak internals?
2. Invariant expression — are illegal states unrepresentable, or are invalid combinations possible?
3. Usefulness — does the type carry meaning beyond a tuple/dict of primitives ("primitive obsession", "stringly-typed")?
4. Enforcement — are constructors, factories, or smart-constructor patterns used to keep invariants intact?
5. Over-design — is the type doing too much, or is it premature abstraction?

Reference exact NEW-side line numbers from the hunk header. If the hunk introduces no new types, you almost certainly have nothing to say. Keep your response to 2-4 bullet points.

If you find nothing worth flagging, respond with exactly: No issues found.

Do not include greetings or sign-offs."#;

/// Defaults for the "code-simplifier" specialist — clarity and unnecessary complexity.
pub const DEFAULT_REVIEW_CODE_SIMPLIFIER_SYSTEM: &str = r#"You are an expert at code clarity and simplification. You review a single diff hunk as one pass of a multi-agent review.

For the given hunk, look only for changes that would make the code meaningfully simpler without changing behavior:
1. Duplicated logic that could reuse an existing helper, or redundant code that restates work already done
2. Convoluted control flow that has a clear, flatter equivalent (needless nesting, double negatives, dead branches)
3. Unnecessary intermediate state, variables, or abstraction that adds no value at this size
4. Over-engineering: premature generalization or indirection for a single caller

Reference exact NEW-side line numbers from the hunk header. Do NOT flag pure style or naming — other specialists own that. Suggest a simplification only when you are confident it preserves behavior and is genuinely clearer; skip subjective rewrites. Keep your response to 2-4 bullet points.

If you find nothing worth flagging, respond with exactly: No issues found.

Do not include greetings or sign-offs."#;

/// Defaults for the "design-principles" specialist — SOLID and DRY at the structural level.
pub const DEFAULT_REVIEW_DESIGN_PRINCIPLES_SYSTEM: &str = r#"You are an expert software designer who evaluates code against the SOLID principles and DRY. You review a single diff hunk as one pass of a multi-agent review.

For the given hunk, flag only high-confidence, consequential design violations:
1. Single Responsibility — a function, class, or module taking on multiple unrelated responsibilities or reasons to change
2. Open/Closed & Liskov — extensions that force edits to existing conditionals/switch ladders instead of adding to them, or subtypes that break their base type's contract
3. Interface Segregation & Dependency Inversion — fat interfaces forcing clients to depend on things they do not use, or high-level logic coupled directly to concrete low-level details that should sit behind an abstraction
4. DRY — logic or domain knowledge duplicated across functions/files that should be unified behind one shared abstraction (especially duplication this hunk introduces against code visible in the surrounding context)

Reference exact NEW-side line numbers from the hunk header. Judge against the code's actual scale — do NOT demand abstraction for a single caller or reward indirection added "just in case"; that is over-engineering, not a SOLID win. Local hunk-level redundancy, naming, and clarity are owned by other specialists — focus on structural design and cross-cutting duplication. Keep your response to 2-4 bullet points.

If you find nothing worth flagging, respond with exactly: No issues found.

Do not include greetings or sign-offs."#;

/// Resolve a prompt: returns the user override from SQLite if present, otherwise the default.
pub async fn resolve_prompt(conn: &libsql::Connection, key: PromptKey) -> Result<String, AppError> {
    let stored = crate::cache::get_setting(conn, &key.db_key()).await?;
    Ok(stored
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| key.default_text().to_string()))
}

/// Persist a user override for a prompt.
pub async fn save_prompt(
    conn: &libsql::Connection,
    key: PromptKey,
    value: &str,
) -> Result<(), AppError> {
    crate::cache::set_setting(conn, &key.db_key(), value).await
}

/// Remove the user override for a prompt (revert to default).
pub async fn reset_prompt(conn: &libsql::Connection, key: PromptKey) -> Result<(), AppError> {
    crate::cache::delete_setting(conn, &key.db_key()).await
}

/// Read the per-prompt provider/model override, if any.
/// Empty / missing means "use the default provider and model from the AI tab".
pub async fn resolve_model_override(
    conn: &libsql::Connection,
    key: PromptKey,
) -> Result<Option<PromptModelOverride>, AppError> {
    let stored = crate::cache::get_setting(conn, &key.model_db_key()).await?;
    let Some(stored) = stored.filter(|s| !s.is_empty()) else {
        return Ok(None);
    };

    let model_override = match serde_json::from_str::<PromptModelOverride>(&stored) {
        Ok(mut parsed) => {
            parsed.provider_id = parsed
                .provider_id
                .map(|id| id.trim().to_string())
                .filter(|id| !id.is_empty());
            parsed.model = parsed.model.trim().to_string();
            parsed
        }
        Err(_) => PromptModelOverride {
            provider_id: crate::cache::get_setting(conn, &key.provider_db_key())
                .await?
                .map(|id| id.trim().to_string())
                .filter(|id| !id.is_empty()),
            model: stored.trim().to_string(),
        },
    };

    if model_override.model.is_empty() {
        Ok(None)
    } else {
        Ok(Some(model_override))
    }
}

/// Backward-compatible helper for call sites that only need the model string.
pub async fn resolve_model(
    conn: &libsql::Connection,
    key: PromptKey,
) -> Result<Option<String>, AppError> {
    Ok(resolve_model_override(conn, key).await?.map(|o| o.model))
}

pub async fn save_model(
    conn: &libsql::Connection,
    key: PromptKey,
    model: &str,
) -> Result<(), AppError> {
    save_model_override(conn, key, None, model).await
}

pub async fn save_model_override(
    conn: &libsql::Connection,
    key: PromptKey,
    provider_id: Option<&str>,
    model: &str,
) -> Result<(), AppError> {
    let model = model.trim();
    if model.is_empty() {
        return reset_model(conn, key).await;
    }
    let provider_id = provider_id
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_string);
    let payload = serde_json::to_string(&PromptModelOverride {
        provider_id: provider_id.clone(),
        model: model.to_string(),
    })
    .map_err(|e| AppError::Ai(format!("Failed to serialize prompt model override: {}", e)))?;
    crate::cache::set_setting(conn, &key.model_db_key(), &payload).await?;
    match provider_id {
        Some(id) => crate::cache::set_setting(conn, &key.provider_db_key(), &id).await,
        None => crate::cache::delete_setting(conn, &key.provider_db_key()).await,
    }
}

pub async fn reset_model(conn: &libsql::Connection, key: PromptKey) -> Result<(), AppError> {
    crate::cache::delete_setting(conn, &key.model_db_key()).await?;
    crate::cache::delete_setting(conn, &key.provider_db_key()).await
}

/// Prompt for generating explain-hunk user message.
pub fn explain_hunk_user(file_path: &str, hunk_header: &str, hunk_text: &str) -> String {
    format!(
        "Please explain this hunk from `{}`:\n\n{}\n{}",
        file_path, hunk_header, hunk_text
    )
}
