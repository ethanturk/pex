use crate::ai::prompts::{resolve_prompt, PromptKey, PromptModelOverride};
use crate::ai::{AiProvider, ChatMessage, ChatRole, ToolCall, ToolChatMessage, ToolDefinition};
use crate::diff::engine::extract_hunks;
use crate::review::prompts;
use crate::review::rules::ReviewRuleMatch;
use crate::review::state::{self, ReviewMode, ReviewState};
use crate::AppError;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::Emitter;
use tokio::sync::Semaphore;

#[derive(Clone)]
struct SpecialistPrompt {
    key: PromptKey,
    system_prompt: String,
    provider: Arc<dyn AiProvider>,
    model_override: Option<String>,
}

fn cancelled(flag: &AtomicBool) -> Result<(), AppError> {
    if flag.load(Ordering::SeqCst) {
        Err(AppError::Provider("Review cancelled".into()))
    } else {
        Ok(())
    }
}

fn resolve_prompt_provider(
    providers: &[crate::ai::AiProviderConfig],
    default_provider_id: &str,
    model_override: Option<&PromptModelOverride>,
    default_runtime_provider: &Arc<dyn AiProvider>,
) -> Result<Arc<dyn AiProvider>, AppError> {
    let selected_provider_id = model_override
        .and_then(|m| m.provider_id.as_deref())
        .unwrap_or(default_provider_id);
    if selected_provider_id == default_provider_id {
        return Ok(default_runtime_provider.clone());
    }

    let provider = providers
        .iter()
        .find(|p| p.id == selected_provider_id)
        .ok_or_else(|| AppError::Ai(format!("AI provider not found: {}", selected_provider_id)))?;
    let api_key = crate::ai::read_ai_provider_api_key(provider)?
        .ok_or_else(|| AppError::Ai(format!("API key not configured for {}", provider.name)))?;
    crate::ai::provider_from_config(provider, &api_key)
}

/// Input for a PR review: the files and their content.
#[derive(Debug, Clone)]
pub struct ReviewInput {
    pub pr_key: String,
    pub pr_title: String,
    pub files: Vec<FileInput>,
    pub standards: String,
    pub project_id: String,
    pub repo_id: String,
    pub pr_id: i64,
    pub mode: ReviewMode,
    /// Thorough mode: the specialist prompt keys (as returned by
    /// `PromptKey::as_str`) the user chose to run. `None` runs the full set
    /// (used by resume, the eval harness, and any non-interactive caller).
    pub enabled_specialists: Option<Vec<String>>,
    #[allow(dead_code)]
    pub rules: HashMap<String, ReviewRuleMatch>,
    #[allow(dead_code)]
    pub related_files: HashMap<String, Vec<String>>,
    /// Compiled deterministic AST rules from the repo's `.pex/ast-rules.yml`,
    /// run alongside the built-in stock rules. `None` if the repo has none.
    /// `Arc` so `ReviewInput` stays cheap to clone (the matchers aren't `Clone`).
    pub ast_rules: Option<std::sync::Arc<crate::review::deterministic::CompiledRuleSet>>,
}

#[derive(Debug, Clone)]
pub struct FileInput {
    pub path: String,
    pub old_content: String,
    pub new_content: String,
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Critical,
    Moderate,
    Minor,
}

/// Confidence (0–100) assigned to findings that predate explicit scoring or
/// that the model failed to score. Set to the default reporting threshold so a
/// missing score surfaces exactly as findings did before confidence existed,
/// rather than being silently dropped.
pub fn default_confidence() -> u8 {
    crate::ai::DEFAULT_CONFIDENCE_THRESHOLD
}

/// Triage tier for a finding, derived deterministically from severity,
/// confidence, and whether it is line-anchored (a proxy for blast radius —
/// file-level findings can't be actioned on a specific line). Drives ordering,
/// which findings are "pulled forward" as individual comments, and which are
/// "pushed back" into a single rollup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Tier {
    /// High-impact, high-confidence — surfaced first, posted individually.
    Blocking,
    /// Real and worth fixing, but not gating.
    ShouldFix,
    /// Low-impact polish — collapsed into a rollup so it never buries signal.
    Nit,
    /// File-level / informational — no specific line to act on.
    Fyi,
}

impl Tier {
    /// Lower rank sorts first (Blocking → FYI).
    pub fn rank(self) -> u8 {
        match self {
            Tier::Blocking => 0,
            Tier::ShouldFix => 1,
            Tier::Nit => 2,
            Tier::Fyi => 3,
        }
    }

    /// Whether this tier is "pulled forward": surfaced prominently and posted
    /// as its own comment. The rest are "pushed back" into a single rollup.
    pub fn is_actionable(self) -> bool {
        matches!(self, Tier::Blocking | Tier::ShouldFix)
    }

    pub fn label(self) -> &'static str {
        match self {
            Tier::Blocking => "Blocking",
            Tier::ShouldFix => "Should fix",
            Tier::Nit => "Nit",
            Tier::Fyi => "FYI",
        }
    }
}

/// Default tier used only when deserializing a finding that lacks one. Defaults
/// to `ShouldFix` so an un-tiered finding is treated as actionable rather than
/// silently hidden in the rollup.
pub fn default_tier() -> Tier {
    Tier::ShouldFix
}

/// Compute the triage tier for a finding. Critical findings are always
/// actionable (Blocking when confidence is at/above the configurable
/// `blocking_confidence` "critical line", else Should-fix) regardless of
/// anchor; non-critical findings with no line anchor are informational (FYI).
pub fn tier_for(
    severity: Severity,
    confidence: u8,
    line_start: Option<usize>,
    blocking_confidence: u8,
) -> Tier {
    match severity {
        Severity::Critical => {
            if confidence >= blocking_confidence {
                Tier::Blocking
            } else {
                Tier::ShouldFix
            }
        }
        _ if line_start.is_none() => Tier::Fyi,
        Severity::Moderate if confidence >= 80 => Tier::ShouldFix,
        _ => Tier::Nit,
    }
}

/// A single review finding produced by the engine. Each finding is intended to
/// become one ADO comment, anchored to a line range when possible.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    pub file_path: String,
    pub severity: Severity,
    /// How sure the reviewer is the finding is real (0–100), distinct from
    /// `severity` (how bad it is if real).
    #[serde(default = "default_confidence")]
    pub confidence: u8,
    /// Triage tier (Blocking → FYI), derived from severity + confidence + anchor.
    #[serde(default = "default_tier")]
    pub tier: Tier,
    /// Specialist label(s) that raised this finding (e.g. "silent-failure-hunter").
    /// Drives per-specialist calibration. Empty when unattributed (e.g. Fast mode).
    #[serde(default)]
    pub sources: Vec<String>,
    pub line_start: Option<usize>,
    pub line_end: Option<usize>,
    pub comment: String,
}

/// Per-file aggregate result parsed from the file-aggregate LLM response.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FileAggregateResult {
    pub summary: String,
    pub verdict: String,
    pub findings: Vec<FileAggregateFinding>,
}

/// Same shape as `Finding` but without `file_path` — the engine injects the
/// path from the file being aggregated.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileAggregateFinding {
    pub severity: Severity,
    /// Adjudicator confidence (0–100). Defaulted for back-compat with states
    /// persisted before confidence scoring existed.
    #[serde(default = "default_confidence")]
    pub confidence: u8,
    pub line_start: Option<usize>,
    pub line_end: Option<usize>,
    pub comment: String,
    /// Exact current-file snippet used to relocate the inline anchor. Required
    /// for line-level findings; file-level findings keep this null/empty.
    #[serde(default)]
    pub existing_code: Option<String>,
    /// New-side line(s) the adjudicator cited to justify the finding. Used by
    /// the deterministic anchor check and for logging; stripped before posting.
    #[serde(default)]
    pub evidence: Option<String>,
    /// Specialist label(s) the adjudicator says raised this finding, echoed from
    /// the `[label]` tags on the per-hunk candidates. Validated against the known
    /// specialist set after parsing. Drives per-specialist calibration.
    #[serde(default)]
    pub sources: Vec<String>,
}

/// The complete review output.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ReviewOutput {
    pub summary: String,
    pub findings: Vec<Finding>,
}

/// Best-effort JSON extraction from an LLM response. Strips ``` fences and
/// trims surrounding prose; returns the parsed result or an error string.
pub fn parse_file_aggregate(raw: &str) -> Result<FileAggregateResult, String> {
    let trimmed = raw.trim();
    // Strip leading/trailing code fences if the model ignored "no fences".
    let inner = if let Some(stripped) = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
    {
        stripped
            .trim_start_matches('\n')
            .trim_end_matches("```")
            .trim()
    } else {
        trimmed
    };
    // Fallback: grab the first {...} block in case the model added prose.
    let json_str = if inner.starts_with('{') {
        inner.to_string()
    } else if let (Some(start), Some(end)) = (inner.find('{'), inner.rfind('}')) {
        inner[start..=end].to_string()
    } else {
        return Err(format!("no JSON object found in response: {}", inner));
    };
    serde_json::from_str::<FileAggregateResult>(&json_str)
        .map_err(|e| format!("JSON parse error: {} — body was: {}", e, json_str))
}

/// Deterministic, no-LLM precision guards applied to a freshly-adjudicated
/// file aggregate. This sync variant is used by tests and headless callers that
/// do not want a relocation LLM call:
///   1. drop findings whose `confidence` is below `threshold`;
///   2. for line-level findings, require `existingCode` and resolve it to a
///      new-side line range by snippet matching hunks first, then the full file.
/// File-level findings (`line_start == None` and `line_end == None`) are exempt
/// from anchoring. Returns the number of findings dropped.
pub fn apply_finding_guards(
    aggregate: &mut FileAggregateResult,
    file_path: &str,
    threshold: u8,
    hunks: &[crate::diff::engine::DiffHunk],
    new_content: &str,
) -> usize {
    let before = aggregate.findings.len();
    let mut kept = Vec::with_capacity(aggregate.findings.len());
    for mut f in aggregate.findings.drain(..) {
        if f.confidence < threshold {
            eprintln!(
                "[review] dropped finding in {} (confidence {} < threshold {})",
                file_path, f.confidence, threshold
            );
            continue;
        }
        if f.line_start.is_none() && f.line_end.is_none() {
            kept.push(f);
            continue;
        }
        let Some(existing_code) = f.existing_code.as_deref().filter(|s| !s.trim().is_empty())
        else {
            eprintln!(
                "[review] dropped finding in {} (missing existingCode for line-level finding)",
                file_path
            );
            continue;
        };
        if let Some(anchor) = crate::review::anchoring::resolve_existing_code(
            existing_code,
            new_content,
            hunks,
            f.line_start,
        ) {
            f.line_start = Some(anchor.line_start);
            f.line_end = Some(anchor.line_end);
            kept.push(f);
        } else {
            eprintln!(
                "[review] dropped finding in {} (existingCode could not be resolved)",
                file_path
            );
        }
    }
    aggregate.findings = kept;
    before - aggregate.findings.len()
}

#[allow(clippy::too_many_arguments)]
async fn apply_finding_guards_with_relocation(
    provider: &Arc<dyn AiProvider>,
    aggregate: &mut FileAggregateResult,
    file_path: &str,
    threshold: u8,
    hunks: &[crate::diff::engine::DiffHunk],
    new_content: &str,
    retry_count: u32,
    diag: &crate::review::diagnostics::Diagnostics,
) -> usize {
    let before = aggregate.findings.len();
    let mut kept = Vec::with_capacity(aggregate.findings.len());

    for mut f in aggregate.findings.drain(..) {
        if f.confidence < threshold {
            emit_anchor_drop(diag, file_path, &f, "below_threshold", None);
            continue;
        }
        if f.line_start.is_none() && f.line_end.is_none() {
            kept.push(f);
            continue;
        }
        if f.line_start.is_none() || f.line_end.is_none() {
            emit_anchor_drop(diag, file_path, &f, "invalid_line_range", None);
            continue;
        }
        let Some(existing_code) = f.existing_code.as_deref().filter(|s| !s.trim().is_empty())
        else {
            emit_anchor_drop(diag, file_path, &f, "missing_existing_code", None);
            continue;
        };

        if let Some(anchor) = crate::review::anchoring::resolve_existing_code(
            existing_code,
            new_content,
            hunks,
            f.line_start,
        ) {
            let original_line = f.line_start;
            f.line_start = Some(anchor.line_start);
            f.line_end = Some(anchor.line_end);
            if diag.is_enabled() {
                diag.event(
                    "anchor_resolved",
                    serde_json::json!({
                        "filePath": file_path,
                        "originalLineStart": original_line,
                        "resolvedLineStart": f.line_start,
                        "resolvedLineEnd": f.line_end,
                        "source": format!("{:?}", anchor.source),
                        "normalized": anchor.normalized,
                        "confidence": f.confidence,
                        "comment": f.comment,
                    }),
                );
            }
            kept.push(f);
            continue;
        }

        let started = std::time::Instant::now();
        match relocate_anchor(provider, file_path, new_content, &f, retry_count).await {
            Ok(Some(snippet)) => {
                if let Some(anchor) = crate::review::anchoring::resolve_existing_code(
                    &snippet,
                    new_content,
                    hunks,
                    f.line_start,
                ) {
                    let original_line = f.line_start;
                    f.existing_code = Some(snippet);
                    f.line_start = Some(anchor.line_start);
                    f.line_end = Some(anchor.line_end);
                    if diag.is_enabled() {
                        diag.event(
                            "anchor_relocated",
                            serde_json::json!({
                                "filePath": file_path,
                                "originalLineStart": original_line,
                                "resolvedLineStart": f.line_start,
                                "resolvedLineEnd": f.line_end,
                                "source": format!("{:?}", anchor.source),
                                "normalized": anchor.normalized,
                                "latencyMs": started.elapsed().as_millis() as u64,
                                "confidence": f.confidence,
                                "comment": f.comment,
                            }),
                        );
                    }
                    kept.push(f);
                } else {
                    emit_anchor_drop(
                        diag,
                        file_path,
                        &f,
                        "relocation_unresolved",
                        Some(started.elapsed()),
                    );
                }
            }
            Ok(None) => emit_anchor_drop(
                diag,
                file_path,
                &f,
                "relocation_empty",
                Some(started.elapsed()),
            ),
            Err(e) => emit_anchor_drop(
                diag,
                file_path,
                &f,
                &format!("relocation_failed: {}", e),
                Some(started.elapsed()),
            ),
        }
    }

    aggregate.findings = kept;
    before - aggregate.findings.len()
}

fn emit_anchor_drop(
    diag: &crate::review::diagnostics::Diagnostics,
    file_path: &str,
    finding: &FileAggregateFinding,
    reason: &str,
    latency: Option<std::time::Duration>,
) {
    if !diag.is_enabled() {
        return;
    }
    let mut payload = serde_json::json!({
        "filePath": file_path,
        "severity": finding.severity,
        "confidence": finding.confidence,
        "lineStart": finding.line_start,
        "lineEnd": finding.line_end,
        "sources": &finding.sources,
        "comment": &finding.comment,
        "reason": reason,
    });
    if let (Some(obj), Some(latency)) = (payload.as_object_mut(), latency) {
        obj.insert(
            "latencyMs".to_string(),
            serde_json::json!(latency.as_millis() as u64),
        );
    }
    diag.event("anchor_drop", payload);
}

async fn relocate_anchor(
    provider: &Arc<dyn AiProvider>,
    file_path: &str,
    new_content: &str,
    finding: &FileAggregateFinding,
    retry_count: u32,
) -> Result<Option<String>, AppError> {
    let messages = vec![
        ChatMessage {
            role: ChatRole::System,
            content: prompts::ANCHOR_RELOCATION_SYSTEM.to_string(),
        },
        ChatMessage {
            role: ChatRole::User,
            content: prompts::anchor_relocation_user_message(
                file_path,
                &finding.comment,
                finding.evidence.as_deref(),
                finding.line_start,
                new_content,
            ),
        },
    ];
    let raw = chat_with_retries(provider, &messages, retry_count).await?;
    let snippet = crate::review::anchoring::extract_fenced_snippet(&raw);
    if snippet.trim().is_empty() {
        Ok(None)
    } else {
        Ok(Some(snippet))
    }
}

/// The canonical specialist labels (the closed vocabulary the adjudicator may
/// cite in a finding's `sources`). Used to validate LLM-reported attribution.
fn known_specialist_labels() -> Vec<&'static str> {
    PromptKey::THOROUGH_SPECIALISTS
        .iter()
        .map(|k| k.specialist_label())
        .collect()
}

/// Clean up adjudicator-reported `sources`: lowercase/trim, keep only known
/// specialist labels, dedupe. A hallucinated or empty label can't pollute the
/// per-specialist calibration; findings with no valid source stay empty and are
/// bucketed as "unattributed" downstream.
pub fn normalize_finding_sources(aggregate: &mut FileAggregateResult) {
    let known = known_specialist_labels();
    for f in &mut aggregate.findings {
        let mut cleaned: Vec<String> = Vec::new();
        for s in f.sources.drain(..) {
            let s = s.trim().to_ascii_lowercase();
            if known.contains(&s.as_str()) && !cleaned.contains(&s) {
                cleaned.push(s);
            }
        }
        f.sources = cleaned;
    }
}

fn hunk_changed_lines(hunks: &[crate::diff::engine::DiffHunk]) -> usize {
    hunks
        .iter()
        .flat_map(|h| h.lines.iter())
        .filter(|line| line.kind == "+")
        .count()
}

fn format_rule_context(rule: &ReviewRuleMatch) -> String {
    format!("{}:\n{}", rule.title, rule.rule)
}

fn format_related_context(related: &[String]) -> Option<String> {
    if related.is_empty() {
        None
    } else {
        Some(format!(
            "Related changed files for context only: {}",
            related.join(", ")
        ))
    }
}

fn build_review_guidance(
    standards: &str,
    rule_context: Option<&str>,
    related_context: Option<&str>,
    file_review_context: Option<&str>,
) -> String {
    let mut sections = Vec::new();
    if !standards.trim().is_empty() {
        sections.push(format!("Project standards:\n{}", standards.trim()));
    }
    if let Some(rule_context) = rule_context.filter(|s| !s.trim().is_empty()) {
        sections.push(format!(
            "Path-specific review checklist:\n{}",
            rule_context.trim()
        ));
    }
    if let Some(related_context) = related_context.filter(|s| !s.trim().is_empty()) {
        sections.push(related_context.trim().to_string());
    }
    if let Some(file_review_context) = file_review_context.filter(|s| !s.trim().is_empty()) {
        sections.push(format!(
            "Additional file context gathered before review:\n{}",
            file_review_context.trim()
        ));
    }
    sections.join("\n\n")
}

#[allow(clippy::too_many_arguments)]
async fn build_file_review_context(
    provider: &Arc<dyn AiProvider>,
    file: &FileInput,
    hunks: &[crate::diff::engine::DiffHunk],
    file_entries: &[(FileInput, Vec<crate::diff::engine::DiffHunk>)],
    related_files: &[String],
    rule_context: Option<&str>,
    _retry_count: u32,
    diag: &crate::review::diagnostics::Diagnostics,
) -> Result<Option<String>, AppError> {
    let changed_files: HashMap<String, (&FileInput, &[crate::diff::engine::DiffHunk])> =
        file_entries
            .iter()
            .map(|(input, hunks)| (input.path.clone(), (input, hunks.as_slice())))
            .collect();
    let mut messages = vec![
        ToolChatMessage::Message(ChatMessage {
            role: ChatRole::System,
            content: "You are planning a bounded code review context pass. Use tools only when they help locate changed-file context for this file. Finish by calling task_done with a concise context summary, or answer with that summary if no tools are needed.".to_string(),
        }),
        ToolChatMessage::Message(ChatMessage {
            role: ChatRole::User,
            content: file_context_plan_prompt(file, hunks, related_files, rule_context),
        }),
    ];
    let tools = review_tool_definitions();
    let mut last_content = String::new();

    for round in 0..4 {
        let started = std::time::Instant::now();
        let response = match provider.chat_with_tools(&messages, &tools, None).await {
            Ok(response) => response,
            Err(e) => {
                if diag.is_enabled() {
                    diag.event(
                        "file_context_plan",
                        serde_json::json!({
                            "filePath": file.path,
                            "round": round,
                            "latencyMs": started.elapsed().as_millis() as u64,
                            "error": e.to_string(),
                        }),
                    );
                }
                return Ok(None);
            }
        };
        if diag.is_enabled() {
            diag.event(
                "file_context_plan",
                serde_json::json!({
                    "filePath": file.path,
                    "round": round,
                    "latencyMs": started.elapsed().as_millis() as u64,
                    "content": &response.content,
                    "toolCalls": response.tool_calls.iter().map(|c| &c.name).collect::<Vec<_>>(),
                }),
            );
        }
        last_content = response.content.clone();
        if response.tool_calls.is_empty() {
            return Ok(non_empty_context(&last_content));
        }
        messages.push(ToolChatMessage::AssistantToolCalls {
            content: if response.content.trim().is_empty() {
                None
            } else {
                Some(response.content)
            },
            tool_calls: response.tool_calls.clone(),
        });

        let mut done_summary: Option<String> = None;
        for call in response.tool_calls {
            let tool_started = std::time::Instant::now();
            let result = execute_review_tool(&call, &changed_files);
            if diag.is_enabled() {
                let (content_for_diag, error_for_diag) = match &result {
                    Ok(executed) => (Some(cap_tool_output(&executed.content, 600)), None),
                    Err(e) => (None, Some(e.to_string())),
                };
                diag.event(
                    "tool_call",
                    serde_json::json!({
                        "filePath": file.path,
                        "name": &call.name,
                        "arguments": &call.arguments,
                        "latencyMs": tool_started.elapsed().as_millis() as u64,
                        "result": content_for_diag,
                        "error": error_for_diag,
                    }),
                );
            }
            let content = match result {
                Ok(executed) => {
                    if executed.done {
                        done_summary = non_empty_context(&executed.content);
                    }
                    executed.content
                }
                Err(e) => format!("tool error: {}", e),
            };
            messages.push(ToolChatMessage::ToolResult {
                tool_call_id: call.id,
                name: call.name,
                content,
            });
        }
        if done_summary.is_some() {
            return Ok(done_summary);
        }
    }

    Ok(non_empty_context(&last_content))
}

fn file_context_plan_prompt(
    file: &FileInput,
    hunks: &[crate::diff::engine::DiffHunk],
    related_files: &[String],
    rule_context: Option<&str>,
) -> String {
    let mut msg = format!(
        "Review target file: `{}`\nChanged lines: {}\nHunks: {}\n",
        file.path,
        hunk_changed_lines(hunks),
        hunks.len()
    );
    if !related_files.is_empty() {
        msg.push_str(&format!(
            "Related changed files: {}\n",
            related_files.join(", ")
        ));
    }
    if let Some(rule_context) = rule_context.filter(|s| !s.trim().is_empty()) {
        msg.push_str(&format!("\nPath checklist:\n{}\n", rule_context));
    }
    msg.push_str(
        "\nUse changed-file tools to gather only context likely to reduce false positives. Do not review unrelated code. Finish with task_done({\"summary\":\"...\"}).",
    );
    msg
}

fn review_tool_definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "file_read".to_string(),
            description: "Read a bounded range from a changed file's new-side content.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "startLine": { "type": "integer", "minimum": 1 },
                    "endLine": { "type": "integer", "minimum": 1 }
                },
                "required": ["path"]
            }),
        },
        ToolDefinition {
            name: "file_read_diff".to_string(),
            description: "Read the structured diff hunks for a changed file.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": { "path": { "type": "string" } },
                "required": ["path"]
            }),
        },
        ToolDefinition {
            name: "file_find_changed".to_string(),
            description: "Find changed file paths containing a substring.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": { "pattern": { "type": "string" } },
                "required": ["pattern"]
            }),
        },
        ToolDefinition {
            name: "code_search_changed".to_string(),
            description: "Search changed new-side file content for a substring.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": { "query": { "type": "string" } },
                "required": ["query"]
            }),
        },
        ToolDefinition {
            name: "task_done".to_string(),
            description: "Finish the context pass with a concise summary.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": { "summary": { "type": "string" } },
                "required": ["summary"]
            }),
        },
    ]
}

struct ToolExecution {
    content: String,
    done: bool,
}

fn execute_review_tool(
    call: &ToolCall,
    changed_files: &HashMap<String, (&FileInput, &[crate::diff::engine::DiffHunk])>,
) -> Result<ToolExecution, AppError> {
    match call.name.as_str() {
        "file_read" => {
            let path = tool_arg_str(&call.arguments, "path")?.trim_start_matches('/');
            let (file, _) = changed_files
                .get(path)
                .ok_or_else(|| AppError::Ai(format!("changed file not found: {}", path)))?;
            let lines: Vec<&str> = file.new_content.lines().collect();
            let start = tool_arg_usize(&call.arguments, "startLine")
                .unwrap_or(1)
                .max(1);
            let end = tool_arg_usize(&call.arguments, "endLine").unwrap_or(lines.len());
            let start_idx = start.saturating_sub(1).min(lines.len());
            let end_idx = end.min(lines.len());
            let mut out = String::new();
            for (offset, line) in lines[start_idx..end_idx].iter().enumerate() {
                out.push_str(&format!("{}\t{}\n", start_idx + offset + 1, line));
            }
            Ok(ToolExecution {
                content: cap_tool_output(&out, 8000),
                done: false,
            })
        }
        "file_read_diff" => {
            let path = tool_arg_str(&call.arguments, "path")?.trim_start_matches('/');
            let (_, hunks) = changed_files
                .get(path)
                .ok_or_else(|| AppError::Ai(format!("changed file not found: {}", path)))?;
            Ok(ToolExecution {
                content: cap_tool_output(&render_hunks_for_tool(hunks), 8000),
                done: false,
            })
        }
        "file_find_changed" => {
            let pattern = tool_arg_str(&call.arguments, "pattern")?.to_ascii_lowercase();
            let mut matches: Vec<&String> = changed_files
                .keys()
                .filter(|path| path.to_ascii_lowercase().contains(&pattern))
                .collect();
            matches.sort();
            Ok(ToolExecution {
                content: if matches.is_empty() {
                    "No changed paths matched.".to_string()
                } else {
                    matches
                        .into_iter()
                        .take(50)
                        .cloned()
                        .collect::<Vec<_>>()
                        .join("\n")
                },
                done: false,
            })
        }
        "code_search_changed" => {
            let query = tool_arg_str(&call.arguments, "query")?.to_ascii_lowercase();
            let mut out = String::new();
            for (path, (file, _)) in changed_files {
                for (idx, line) in file.new_content.lines().enumerate() {
                    if line.to_ascii_lowercase().contains(&query) {
                        out.push_str(&format!("{}:{}\t{}\n", path, idx + 1, line));
                        if out.len() > 8000 {
                            break;
                        }
                    }
                }
                if out.len() > 8000 {
                    break;
                }
            }
            Ok(ToolExecution {
                content: if out.trim().is_empty() {
                    "No changed content matched.".to_string()
                } else {
                    cap_tool_output(&out, 8000)
                },
                done: false,
            })
        }
        "task_done" => Ok(ToolExecution {
            content: tool_arg_str(&call.arguments, "summary")?.to_string(),
            done: true,
        }),
        other => Err(AppError::Ai(format!("unknown review tool: {}", other))),
    }
}

fn tool_arg_str<'a>(args: &'a serde_json::Value, key: &str) -> Result<&'a str, AppError> {
    args.get(key)
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .map(str::trim)
        .ok_or_else(|| AppError::Ai(format!("missing tool argument `{}`", key)))
}

fn tool_arg_usize(args: &serde_json::Value, key: &str) -> Option<usize> {
    args.get(key)
        .and_then(|v| v.as_u64())
        .and_then(|n| usize::try_from(n).ok())
}

fn render_hunks_for_tool(hunks: &[crate::diff::engine::DiffHunk]) -> String {
    let mut out = String::new();
    for hunk in hunks {
        out.push_str(&format!("{}\n", hunk.header));
        for line in &hunk.lines {
            let line_no = line
                .new_lineno
                .or(line.old_lineno)
                .map(|n| n.to_string())
                .unwrap_or_else(|| "-".to_string());
            out.push_str(&format!("{}\t{}{}\n", line_no, line.kind, line.content));
        }
        out.push('\n');
    }
    out
}

fn cap_tool_output(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max_chars).collect();
    out.push_str("\n[truncated]");
    out
}

fn non_empty_context(content: &str) -> Option<String> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(cap_tool_output(trimmed, 4000))
    }
}

fn emit_progress(app: &tauri::AppHandle, phase: &str, detail: &str, extra: serde_json::Value) {
    let mut payload = serde_json::json!({
        "phase": phase,
        "detail": detail,
    });
    if let serde_json::Value::Object(ref mut map) = payload {
        if let serde_json::Value::Object(extra_map) = extra {
            for (k, v) in extra_map {
                map.insert(k, v);
            }
        }
    }
    let _ = app.emit("review-progress", &payload);
}

/// Run the full multi-pass review.
///
/// `resume` carries the caller's intent: `true` continues from any saved
/// progress for this PR, `false` starts fresh (and discards stale saved state,
/// e.g. left behind by a cancelled run) so a fresh start never silently resumes.
pub async fn run_review(
    app: tauri::AppHandle,
    provider: Arc<dyn AiProvider>,
    input: ReviewInput,
    conn: &libsql::Connection,
    cancel: Arc<AtomicBool>,
    diag: crate::review::diagnostics::Diagnostics,
    resume: bool,
) -> Result<ReviewOutput, AppError> {
    // ---- Prepare: sort files by hunk count (largest first) ----
    let mut file_entries: Vec<(FileInput, Vec<crate::diff::engine::DiffHunk>)> = input
        .files
        .into_iter()
        .map(|f| {
            let hunks = extract_hunks(&f.old_content, &f.new_content);
            (f, hunks)
        })
        .collect();

    file_entries.retain(|(_, hunks)| !hunks.is_empty());
    file_entries.sort_by(|a, b| b.1.len().cmp(&a.1.len()));

    let file_paths: Vec<String> = file_entries.iter().map(|(f, _)| f.path.clone()).collect();
    let mut state = ReviewState::new(input.pr_key.clone(), file_paths.clone(), input.mode);

    // Resolved once for the run — changing it mid-run isn't worth the surprise
    // factor, and re-reading per call would just thrash the DB lock.
    let retry_count = crate::ai::read_retry_count(conn)
        .await
        .unwrap_or(crate::ai::DEFAULT_RETRY_COUNT);
    let hunk_concurrency = crate::ai::read_hunk_concurrency(conn)
        .await
        .unwrap_or(crate::ai::DEFAULT_HUNK_CONCURRENCY)
        .max(1) as usize;
    let llm_permits = Arc::new(Semaphore::new(hunk_concurrency));

    // Minimum confidence a finding must reach to survive the deterministic
    // guard applied after each file's adjudication. Resolved once per run.
    let confidence_threshold = crate::ai::read_confidence_threshold(conn)
        .await
        .unwrap_or(crate::ai::DEFAULT_CONFIDENCE_THRESHOLD);
    // The "critical line": confidence at/above which a Critical finding tiers
    // Blocking. Resolved once per run so it stays stable across the synthesis.
    let blocking_confidence = crate::ai::read_blocking_confidence(conn)
        .await
        .unwrap_or(crate::ai::DEFAULT_BLOCKING_CONFIDENCE);

    if diag.is_enabled() {
        diag.event(
            "run_start",
            serde_json::json!({
                "prKey": input.pr_key,
                "prTitle": input.pr_title,
                "mode": input.mode,
                "fileCount": file_entries.len(),
                "settings": {
                    "confidenceThreshold": confidence_threshold,
                    "blockingConfidence": blocking_confidence,
                    "retryCount": retry_count,
                    "hunkConcurrency": hunk_concurrency,
                },
            }),
        );
        if let Some(p) = diag.path() {
            eprintln!("[diagnostics] writing review trace to {p}");
        }
    }

    // Resolve specialist system prompts + per-specialist model overrides once for
    // the run (Thorough mode only). Resolved up front so user edits in Settings
    // take effect on the next run without restarting the app.
    //
    // Each entry carries the specialist prompt, provider, and optional model
    // override. No override means: fall back to that provider's configured model.
    // Which specialists to run. An explicit (non-empty) selection narrows the
    // set; `None` or a selection that matches nothing falls back to the full
    // roster so a stale/empty list never yields a silent no-op review.
    let enabled: Option<std::collections::HashSet<&str>> = input
        .enabled_specialists
        .as_ref()
        .filter(|v| !v.is_empty())
        .map(|v| v.iter().map(|s| s.as_str()).collect());
    let (default_prompt_provider_id, prompt_providers) = crate::ai::read_ai_provider_configs(conn)
        .await
        .unwrap_or_else(|_| ("default".to_string(), Vec::new()));
    let specialist_prompts: Vec<SpecialistPrompt> = if input.mode == ReviewMode::Thorough {
        let selected: Vec<PromptKey> = PromptKey::THOROUGH_SPECIALISTS
            .iter()
            .copied()
            .filter(|key| {
                enabled
                    .as_ref()
                    .map_or(true, |set| set.contains(key.as_str()))
            })
            .collect();
        // Selection filtered everything out (only unknown keys): run them all.
        let keys = if selected.is_empty() {
            PromptKey::THOROUGH_SPECIALISTS.to_vec()
        } else {
            selected
        };
        let mut prompts = Vec::with_capacity(keys.len());
        for key in keys {
            let text = resolve_prompt(conn, key)
                .await
                .unwrap_or_else(|_| key.default_text().to_string());
            let model_override = crate::ai::prompts::resolve_model_override(conn, key)
                .await
                .unwrap_or(None);
            let sp_provider = resolve_prompt_provider(
                &prompt_providers,
                &default_prompt_provider_id,
                model_override.as_ref(),
                &provider,
            )?;
            prompts.push(SpecialistPrompt {
                key,
                system_prompt: text,
                provider: sp_provider,
                model_override: model_override.map(|m| m.model),
            });
        }
        prompts
    } else {
        Vec::new()
    };

    // Check for resumable state. The persisted state indexes files positionally
    // (`current_file_idx`), so before we trust those indices the freshly built
    // `file_entries` must line up slot-for-slot with the order the state was
    // saved against. That order isn't stable across runs (see
    // `align_to_saved_order`), so realign to the saved order first. If the file
    // set has genuinely drifted since the save, the saved indices are
    // meaningless — discard the state and review fresh rather than re-reviewing
    // already-completed files (the "resume just restarts" bug).
    let mut resuming = false;
    if resume {
        if let Ok(Some(saved)) = state::load_state(conn).await {
            if saved.pr_key == state.pr_key && !saved.is_done() {
                if align_to_saved_order(&mut file_entries, &saved.file_paths) {
                    state = saved;
                    resuming = true;
                } else {
                    let _ = state::clear_state(conn).await;
                }
            }
        }
    } else {
        // Fresh start requested — drop any stale saved state up front so a prior
        // (e.g. cancelled) run for this PR can't silently resume underneath us.
        let _ = state::clear_state(conn).await;
    }
    if resuming {
        emit_progress(
            &app,
            "resume",
            "Resuming from saved progress...",
            serde_json::json!({}),
        );
    }

    // Recompute after any reordering so the plan — and the file slots the loop
    // below indexes into — reflect the actual (possibly realigned) worklist.
    let file_paths: Vec<String> = file_entries.iter().map(|(f, _)| f.path.clone()).collect();

    // Announce the full, ordered worklist up front so the UI can render every
    // file (and tick them off as they complete). `completedCount` lets a resumed
    // run mark already-finished files as done without re-emitting each one.
    // `ruleTitles` carries the deterministic checklist each file matched so the
    // matched rule is visible during the run, not just in the pre-run preview.
    let rule_titles: std::collections::HashMap<&str, &str> = file_paths
        .iter()
        .filter_map(|p| input.rules.get(p).map(|r| (p.as_str(), r.title.as_str())))
        .collect();
    emit_progress(
        &app,
        "plan",
        &format!("Reviewing {} file(s)", file_entries.len()),
        serde_json::json!({
            "files": file_paths.clone(),
            "totalFiles": file_entries.len(),
            "completedCount": state.current_file_idx,
            "ruleTitles": rule_titles,
        }),
    );

    // ---- Phase 1: Hunk Review (per file) ----
    while state.current_file_idx < file_entries.len() {
        cancelled(&cancel)?;
        let file_started = std::time::Instant::now();
        let (file, hunks) = &file_entries[state.current_file_idx];
        let total_hunks = hunks.len();
        // Shared once per file: each hunk pass windows a bounded slice of this
        // for surrounding context, so we clone the Arc, not the string.
        let file_new_content = Arc::new(file.new_content.clone());
        let rule_context = input.rules.get(&file.path).map(format_rule_context);
        let related = input
            .related_files
            .get(&file.path)
            .cloned()
            .unwrap_or_default();
        let related_context = format_related_context(&related);
        let file_review_context = if input.mode == ReviewMode::Thorough
            && (hunk_changed_lines(hunks) >= 50 || hunks.len() > 3)
        {
            build_file_review_context(
                &provider,
                file,
                hunks,
                &file_entries,
                &related,
                rule_context.as_deref(),
                retry_count,
                &diag,
            )
            .await
            .ok()
            .flatten()
        } else {
            None
        };
        let review_guidance = build_review_guidance(
            &input.standards,
            rule_context.as_deref(),
            related_context.as_deref(),
            file_review_context.as_deref(),
        );

        if state.current_file_hunks == 0 {
            state.current_file_hunks = total_hunks;
            state.current_hunk = 0;
            state.current_file_findings.clear();
        }

        emit_progress(
            &app,
            "hunk-review",
            &format!(
                "{} ({}/{})",
                file.path,
                state.current_file_idx + 1,
                file_entries.len()
            ),
            serde_json::json!({
                "fileNum": state.current_file_idx + 1,
                "totalFiles": file_entries.len(),
                "hunk": state.current_hunk,
                "totalHunks": total_hunks,
            }),
        );

        while state.current_hunk < total_hunks {
            cancelled(&cancel)?;

            let batch_start = state.current_hunk;
            let batch_end = (batch_start + hunk_concurrency).min(total_hunks);
            let mut handles = Vec::new();

            for hunk_idx in batch_start..batch_end {
                let provider = provider.clone();
                let hunk = hunks[hunk_idx].clone();
                let file_path = file.path.clone();
                let standards = review_guidance.clone();
                let specialist_prompts = specialist_prompts.clone();
                let llm_permits = llm_permits.clone();
                let file_new_content = file_new_content.clone();
                let mode = input.mode;
                handles.push((
                    hunk_idx,
                    tokio::spawn(async move {
                        review_single_hunk(
                            provider,
                            mode,
                            file_path,
                            hunk_idx,
                            total_hunks,
                            hunk,
                            standards,
                            specialist_prompts,
                            retry_count,
                            llm_permits,
                            file_new_content,
                        )
                        .await
                    }),
                ));
            }

            let mut batch_results = Vec::new();
            for (hunk_idx, handle) in handles {
                let result = match handle.await {
                    Ok(result) => result,
                    Err(e) => Err(AppError::Ai(format!("Hunk review task failed: {}", e))),
                };
                batch_results.push((hunk_idx, result));
            }
            batch_results.sort_by_key(|(hunk_idx, _)| *hunk_idx);

            for (hunk_idx, result) in batch_results {
                let response = match result {
                    Ok(r) => r,
                    Err(e) => {
                        let skip_msg = format!("[skipped — error: {}]", e);
                        emit_progress(
                            &app,
                            "hunk-skipped",
                            &format!(
                                "Hunk {}/{} in {} failed: {}",
                                hunk_idx + 1,
                                total_hunks,
                                file.path,
                                e
                            ),
                            serde_json::json!({}),
                        );
                        state.current_file_findings.push((hunk_idx + 1, skip_msg));
                        state.current_hunk = hunk_idx + 1;
                        save_state_to_db(conn, &state).await;
                        continue;
                    }
                };

                if response.trim() != "No issues found." {
                    if diag.is_enabled() {
                        diag.event(
                            "hunk_candidate",
                            serde_json::json!({
                                "filePath": file.path,
                                "hunk": hunk_idx + 1,
                                "text": response,
                            }),
                        );
                    }
                    state.current_file_findings.push((hunk_idx + 1, response));
                }

                state.current_hunk = hunk_idx + 1;

                emit_progress(
                    &app,
                    "hunk-review",
                    &format!(
                        "{} ({}/{})",
                        file.path,
                        state.current_file_idx + 1,
                        file_entries.len()
                    ),
                    serde_json::json!({
                        "fileNum": state.current_file_idx + 1,
                        "totalFiles": file_entries.len(),
                        "hunk": state.current_hunk,
                        "totalHunks": total_hunks,
                    }),
                );

                save_state_to_db(conn, &state).await;
            }
        }

        // Per-file deterministic anchoring rollup, surfaced on `file-done`.
        let mut kept_count = 0usize;
        let mut anchored_count = 0usize;
        let mut dropped_count = 0usize;

        // ---- File Aggregate ----
        if !state.current_file_findings.is_empty() {
            state.phase = "file-aggregate".into();
            save_state_to_db(conn, &state).await;
            emit_progress(
                &app,
                "file-aggregate",
                &format!("Summarizing {}", file.path),
                serde_json::json!({
                    "fileNum": state.current_file_idx + 1,
                    "totalFiles": file_entries.len(),
                }),
            );

            let agg_messages = vec![
                ChatMessage {
                    role: ChatRole::System,
                    content: prompts::FILE_AGGREGATE_SYSTEM.to_string(),
                },
                ChatMessage {
                    role: ChatRole::User,
                    content: prompts::file_aggregate_user_message(
                        &file.path,
                        &state.current_file_findings,
                        &input.standards,
                        &file.new_content,
                        rule_context.as_deref(),
                        file_review_context.as_deref(),
                    ),
                },
            ];

            let agg_started = std::time::Instant::now();
            let raw = chat_with_retries(&provider, &agg_messages, retry_count)
                .await
                .unwrap_or_else(|e| format!("[aggregate failed — {}]", e));
            if diag.is_enabled() {
                diag.event(
                    "llm_call",
                    serde_json::json!({
                        "stage": "adjudicate",
                        "filePath": file.path,
                        "latencyMs": agg_started.elapsed().as_millis() as u64,
                        "messages": &agg_messages,
                        "response": raw,
                    }),
                );
            }

            let mut aggregate = parse_file_aggregate(&raw).unwrap_or_else(|err| {
                // Log to stderr so the user can see what the model produced.
                eprintln!(
                    "[review] file-aggregate JSON parse failed for {}: {}",
                    file.path, err
                );
                FileAggregateResult {
                    summary: format!("Aggregate parse failed; raw model output: {}", raw),
                    verdict: "review-required".into(),
                    findings: Vec::new(),
                }
            });

            // Deterministic precision guards: drop sub-threshold findings and
            // resolve line-level anchors from exact snippets before they reach
            // the reviewer.
            normalize_finding_sources(&mut aggregate);
            dropped_count = apply_finding_guards_with_relocation(
                &provider,
                &mut aggregate,
                &file.path,
                confidence_threshold,
                hunks,
                &file.new_content,
                retry_count,
                &diag,
            )
            .await;
            // Surviving line-level findings are the ones the anchoring step
            // resolved to a concrete new-side line range.
            anchored_count = aggregate
                .findings
                .iter()
                .filter(|f| f.line_start.is_some())
                .count();
            kept_count = aggregate.findings.len();

            state.completed_files.push((file.path.clone(), aggregate));
        } else {
            state.completed_files.push((
                file.path.clone(),
                FileAggregateResult {
                    summary: "No issues found in this file.".into(),
                    verdict: "approve".into(),
                    findings: Vec::new(),
                },
            ));
        }

        // Deterministic AST checks: produce findings with no LLM, scoped to
        // lines the diff added. They already carry exact line ranges, so they
        // skip the LLM anchoring step entirely. Merge them into this file's
        // aggregate so they flow through tiering, ordering, and suppression
        // alongside the model's findings.
        let det_findings = crate::review::deterministic::check_file(
            &file.path,
            &file.new_content,
            hunks,
            input.ast_rules.as_deref(),
        );
        let deterministic_count = det_findings.len();
        if let Some((_, agg)) = state.completed_files.last_mut() {
            agg.findings.extend(det_findings);
        }

        emit_progress(
            &app,
            "file-done",
            &format!(
                "Reviewed {} — {} finding(s), {} anchored, {} dropped, {} deterministic",
                file.path, kept_count, anchored_count, dropped_count, deterministic_count
            ),
            serde_json::json!({
                "fileIndex": state.current_file_idx,
                "fileNum": state.current_file_idx + 1,
                "totalFiles": file_entries.len(),
                "durationMs": file_started.elapsed().as_millis() as u64,
                "keptFindings": kept_count,
                "anchoredFindings": anchored_count,
                "droppedFindings": dropped_count,
                "deterministicFindings": deterministic_count,
            }),
        );

        state.current_file_idx += 1;
        state.current_file_hunks = 0;
        state.current_hunk = 0;
        state.current_file_findings.clear();
        state.phase = if state.current_file_idx >= file_entries.len() {
            "batch-aggregate".into()
        } else {
            "hunk-review".into()
        };

        save_state_to_db(conn, &state).await;
    }

    // ---- Phase 2: Batch Aggregation ----
    let batch_size = 5;
    let total_batches = state.total_batches;
    if state.current_batch <= total_batches {
        state.phase = "batch-aggregate".into();
        save_state_to_db(conn, &state).await;
    }

    while state.current_batch <= total_batches {
        cancelled(&cancel)?;
        let start = (state.current_batch - 1) * batch_size;
        let end = (start + batch_size).min(state.completed_files.len());

        if start >= state.completed_files.len() {
            break;
        }

        // The batch aggregate prompt only needs the per-file summary string.
        let batch_files: Vec<(String, String)> = state.completed_files[start..end]
            .iter()
            .map(|(path, agg)| (path.clone(), agg.summary.clone()))
            .collect();

        emit_progress(
            &app,
            "batch-aggregate",
            &format!(
                "Batch {}/{} ({} files)",
                state.current_batch,
                total_batches,
                batch_files.len()
            ),
            serde_json::json!({
                "batch": state.current_batch,
                "totalBatches": total_batches,
                "fileCount": batch_files.len(),
                "fileNum": file_entries.len(),
                "totalFiles": file_entries.len(),
            }),
        );

        let batch_messages = vec![
            ChatMessage {
                role: ChatRole::System,
                content: prompts::BATCH_AGGREGATE_SYSTEM.to_string(),
            },
            ChatMessage {
                role: ChatRole::User,
                content: prompts::batch_aggregate_user_message(
                    state.current_batch,
                    total_batches,
                    &batch_files,
                    &input.standards,
                ),
            },
        ];

        let batch_started = std::time::Instant::now();
        let batch_summary = chat_with_retries(&provider, &batch_messages, retry_count)
            .await
            .unwrap_or_else(|e| format!("[batch aggregate failed — {}]", e));
        if diag.is_enabled() {
            diag.event(
                "llm_call",
                serde_json::json!({
                    "stage": "batch",
                    "batch": state.current_batch,
                    "latencyMs": batch_started.elapsed().as_millis() as u64,
                    "messages": &batch_messages,
                    "response": batch_summary,
                }),
            );
        }

        state.batch_summaries.push(batch_summary);
        state.current_batch += 1;

        save_state_to_db(conn, &state).await;
    }

    // ---- Phase 3: Final Synthesis ----
    cancelled(&cancel)?;
    state.phase = "synthesis".into();
    save_state_to_db(conn, &state).await;
    emit_progress(
        &app,
        "synthesis",
        "Producing final review summary...",
        serde_json::json!({
            "fileNum": file_entries.len(),
            "totalFiles": file_entries.len(),
        }),
    );

    // Flatten per-file findings into a single list, injecting the file path
    // onto each one so the frontend can render and post them independently.
    // Compute each finding's triage tier, then order strictly by tier
    // (Blocking first) so high-priority fixes are pulled forward; within a tier,
    // higher confidence first, then by file for locality.
    let mut findings: Vec<Finding> = state
        .completed_files
        .iter()
        .flat_map(|(file_path, agg)| {
            agg.findings.iter().map(move |f| Finding {
                file_path: file_path.clone(),
                severity: f.severity,
                confidence: f.confidence,
                tier: tier_for(f.severity, f.confidence, f.line_start, blocking_confidence),
                sources: f.sources.clone(),
                line_start: f.line_start,
                line_end: f.line_end,
                comment: f.comment.clone(),
            })
        })
        .collect();
    findings.sort_by(|a, b| {
        a.tier
            .rank()
            .cmp(&b.tier.rank())
            .then(b.confidence.cmp(&a.confidence))
            .then_with(|| a.file_path.cmp(&b.file_path))
            .then(a.line_start.cmp(&b.line_start))
    });

    // Suppression memory: drop findings the reviewer previously dismissed on
    // this PR so they don't re-surface on the next iteration.
    let dismissed = crate::review::feedback::dismissed_fingerprints(conn, &input.pr_key)
        .await
        .unwrap_or_default();
    let suppressed = if dismissed.is_empty() {
        0
    } else {
        // Log suppressions in a borrow-only pass before the retain mutates.
        if diag.is_enabled() {
            for f in &findings {
                let fp = crate::review::feedback::fingerprint(&f.file_path, &f.comment);
                if dismissed.contains(&fp) {
                    diag.event(
                        "suppressed",
                        serde_json::json!({
                            "filePath": f.file_path,
                            "fingerprint": fp,
                            "lineStart": f.line_start,
                            "tier": f.tier,
                            "comment": f.comment,
                        }),
                    );
                }
            }
        }
        let before = findings.len();
        findings.retain(|f| {
            let fp = crate::review::feedback::fingerprint(&f.file_path, &f.comment);
            !dismissed.contains(&fp)
        });
        before - findings.len()
    };
    if suppressed > 0 {
        eprintln!(
            "[review] suppressed {} previously-dismissed finding(s) for {}",
            suppressed, input.pr_key
        );
    }

    let final_messages = vec![
        ChatMessage {
            role: ChatRole::System,
            content: prompts::FINAL_SYNTHESIS_SYSTEM.to_string(),
        },
        ChatMessage {
            role: ChatRole::User,
            content: prompts::final_synthesis_user_message(
                &input.pr_title,
                file_entries.len(),
                &state.batch_summaries,
                &input.standards,
            ),
        },
    ];

    let synth_started = std::time::Instant::now();
    let final_review = chat_with_retries(&provider, &final_messages, retry_count)
        .await
        .unwrap_or_else(|e| format!("[final synthesis failed — {}]", e));
    if diag.is_enabled() {
        diag.event(
            "llm_call",
            serde_json::json!({
                "stage": "synthesis",
                "latencyMs": synth_started.elapsed().as_millis() as u64,
                "messages": &final_messages,
                "response": final_review,
            }),
        );
    }
    let final_review = append_exact_statistics(&final_review, file_entries.len(), &findings);

    state.phase = "done".into();
    state.final_review = Some(final_review.clone());
    clear_state_from_db(conn).await;

    if diag.is_enabled() {
        for f in &findings {
            diag.event(
                "finding_final",
                serde_json::json!({
                    "filePath": f.file_path,
                    "severity": f.severity,
                    "confidence": f.confidence,
                    "tier": f.tier,
                    "lineStart": f.line_start,
                    "lineEnd": f.line_end,
                    "sources": f.sources,
                    "comment": f.comment,
                    "fingerprint": crate::review::feedback::fingerprint(&f.file_path, &f.comment),
                }),
            );
        }
        diag.event(
            "run_done",
            serde_json::json!({
                "totalFiles": file_entries.len(),
                "findings": findings.len(),
                "suppressed": suppressed,
                "blocking": findings.iter().filter(|f| f.tier == Tier::Blocking).count(),
                "shouldFix": findings.iter().filter(|f| f.tier == Tier::ShouldFix).count(),
                "nit": findings.iter().filter(|f| f.tier == Tier::Nit).count(),
                "fyi": findings.iter().filter(|f| f.tier == Tier::Fyi).count(),
            }),
        );
    }

    emit_progress(
        &app,
        "done",
        "Review complete",
        serde_json::json!({
            "totalFiles": file_entries.len(),
            "findingsCount": findings.len(),
            "suppressed": suppressed,
        }),
    );

    Ok(ReviewOutput {
        summary: final_review,
        findings,
    })
}

/// Review a single file end-to-end without Tauri, state persistence, progress
/// events, or resumability: run the hunk passes, adjudicate into a structured
/// file result, and apply the deterministic guards. The live `run_review`
/// inlines an equivalent flow with those concerns layered on; this is the
/// headless entry point used by the eval harness so both share the same hunk,
/// adjudication, and guard logic.
pub async fn review_single_file(
    provider: Arc<dyn AiProvider>,
    mode: ReviewMode,
    file: &FileInput,
    standards: &str,
    confidence_threshold: u8,
    retry_count: u32,
) -> Result<FileAggregateResult, AppError> {
    let hunks = extract_hunks(&file.old_content, &file.new_content);
    if hunks.is_empty() {
        return Ok(FileAggregateResult {
            summary: "No reviewable changes in this file.".into(),
            verdict: "approve".into(),
            findings: Vec::new(),
        });
    }

    let specialist_prompts: Vec<SpecialistPrompt> = if mode == ReviewMode::Thorough {
        PromptKey::THOROUGH_SPECIALISTS
            .iter()
            .map(|k| SpecialistPrompt {
                key: *k,
                system_prompt: k.default_text().to_string(),
                provider: provider.clone(),
                model_override: None,
            })
            .collect()
    } else {
        Vec::new()
    };

    let file_new_content = Arc::new(file.new_content.clone());
    let permits = Arc::new(Semaphore::new(1));

    let mut hunk_findings: Vec<(usize, String)> = Vec::new();
    for (idx, hunk) in hunks.iter().enumerate() {
        let response = review_single_hunk(
            provider.clone(),
            mode,
            file.path.clone(),
            idx,
            hunks.len(),
            hunk.clone(),
            standards.to_string(),
            specialist_prompts.clone(),
            retry_count,
            permits.clone(),
            file_new_content.clone(),
        )
        .await?;
        if response.trim() != "No issues found." {
            hunk_findings.push((idx + 1, response));
        }
    }

    if hunk_findings.is_empty() {
        return Ok(FileAggregateResult {
            summary: "No issues found in this file.".into(),
            verdict: "approve".into(),
            findings: Vec::new(),
        });
    }

    let agg_messages = vec![
        ChatMessage {
            role: ChatRole::System,
            content: prompts::FILE_AGGREGATE_SYSTEM.to_string(),
        },
        ChatMessage {
            role: ChatRole::User,
            content: prompts::file_aggregate_user_message(
                &file.path,
                &hunk_findings,
                standards,
                &file.new_content,
                None,
                None,
            ),
        },
    ];

    let raw = chat_with_retries(&provider, &agg_messages, retry_count).await?;
    let mut aggregate = parse_file_aggregate(&raw).map_err(AppError::Ai)?;
    normalize_finding_sources(&mut aggregate);
    apply_finding_guards(
        &mut aggregate,
        &file.path,
        confidence_threshold,
        &hunks,
        &file.new_content,
    );
    Ok(aggregate)
}

fn append_exact_statistics(summary: &str, files_reviewed: usize, findings: &[Finding]) -> String {
    let without_stats = strip_statistics_section(summary).trim().to_string();
    let critical = findings
        .iter()
        .filter(|f| f.severity == Severity::Critical)
        .count();
    let moderate = findings
        .iter()
        .filter(|f| f.severity == Severity::Moderate)
        .count();
    let minor = findings
        .iter()
        .filter(|f| f.severity == Severity::Minor)
        .count();
    let tier = |t: Tier| findings.iter().filter(|f| f.tier == t).count();

    format!(
        "{}\n\n## Statistics\n- Files reviewed: {}\n- Issues found: {} critical, {} moderate, {} minor\n- Triage: {} blocking, {} should-fix, {} nit, {} FYI",
        without_stats,
        files_reviewed,
        critical,
        moderate,
        minor,
        tier(Tier::Blocking),
        tier(Tier::ShouldFix),
        tier(Tier::Nit),
        tier(Tier::Fyi),
    )
}

fn strip_statistics_section(summary: &str) -> &str {
    if let Some(idx) = summary.find("\n## Statistics") {
        &summary[..idx]
    } else if summary.starts_with("## Statistics") {
        ""
    } else {
        summary
    }
}

async fn chat_with_retries(
    provider: &Arc<dyn AiProvider>,
    messages: &[ChatMessage],
    retries: u32,
) -> Result<String, AppError> {
    chat_with_retries_and_model(provider, messages, None, retries).await
}

#[allow(clippy::too_many_arguments)]
async fn review_single_hunk(
    provider: Arc<dyn AiProvider>,
    mode: ReviewMode,
    file_path: String,
    hunk_idx: usize,
    total_hunks: usize,
    hunk: crate::diff::engine::DiffHunk,
    standards: String,
    specialist_prompts: Vec<SpecialistPrompt>,
    retry_count: u32,
    llm_permits: Arc<Semaphore>,
    file_new_content: Arc<String>,
) -> Result<String, AppError> {
    let hunk_text: String = hunk
        .lines
        .iter()
        .map(|l| format!("{}{}", l.kind, l.content))
        .collect::<Vec<_>>()
        .join("");

    let context_note = prompts::hunk_context_note(&file_path, hunk_idx + 1, total_hunks);
    // Surrounding-file window so the reviewer can see definitions / callers and
    // avoid the most common false positives. Empty for tiny / deletion-only hunks.
    let file_ctx =
        prompts::file_context_window(&file_new_content, &hunk, crate::ai::FILE_CONTEXT_MAX_CHARS);
    let user_msg = prompts::hunk_user_message(&file_path, &hunk.header, &hunk_text, "");

    if mode == ReviewMode::Thorough {
        let mut handles = Vec::new();
        for (idx, specialist) in specialist_prompts.into_iter().enumerate() {
            let provider = specialist.provider.clone();
            let key = specialist.key;
            let sys_text = specialist.system_prompt;
            let model_override = specialist.model_override;
            let standards = standards.clone();
            let context_note = context_note.clone();
            let user_msg = user_msg.clone();
            let file_ctx = file_ctx.clone();
            let llm_permits = llm_permits.clone();
            handles.push((
                idx,
                tokio::spawn(async move {
                    let mut pass_messages = vec![
                        ChatMessage {
                            role: ChatRole::System,
                            content: if standards.is_empty() {
                                sys_text
                            } else {
                                format!("{}\n\nProject standards:\n{}", sys_text, standards)
                            },
                        },
                        ChatMessage {
                            role: ChatRole::User,
                            content: context_note,
                        },
                    ];
                    if !file_ctx.is_empty() {
                        pass_messages.push(ChatMessage {
                            role: ChatRole::User,
                            content: file_ctx,
                        });
                    }
                    pass_messages.push(ChatMessage {
                        role: ChatRole::User,
                        content: user_msg,
                    });
                    let result = match llm_permits.acquire_owned().await {
                        Ok(_permit) => {
                            chat_with_retries_and_model(
                                &provider,
                                &pass_messages,
                                model_override.as_deref(),
                                retry_count,
                            )
                            .await
                        }
                        Err(_) => Err(AppError::Ai("LLM concurrency limiter closed".into())),
                    };
                    (key, result)
                }),
            ));
        }

        let mut pass_results = Vec::new();
        for (idx, handle) in handles {
            let result = match handle.await {
                Ok(result) => result,
                Err(e) => (
                    PromptKey::ReviewCodeReviewerSystem,
                    Err(AppError::Ai(format!(
                        "Specialist review task failed: {}",
                        e
                    ))),
                ),
            };
            pass_results.push((idx, result));
        }
        pass_results.sort_by_key(|(idx, _)| *idx);

        let mut outputs: Vec<String> = Vec::new();
        let mut last_err: Option<AppError> = None;
        for (_, (key, result)) in pass_results {
            match result {
                Ok(r) => {
                    if r.trim() != "No issues found." && !r.trim().is_empty() {
                        outputs.push(format!("[{}]\n{}", key.specialist_label(), r.trim()));
                    }
                }
                Err(e) => {
                    last_err = Some(e);
                }
            }
        }
        if outputs.is_empty() {
            if let Some(e) = last_err {
                Err(e)
            } else {
                Ok("No issues found.".to_string())
            }
        } else {
            Ok(outputs.join("\n\n"))
        }
    } else {
        let mut messages = vec![
            ChatMessage {
                role: ChatRole::System,
                content: format!(
                    "{}\n\n{}",
                    prompts::REVIEW_HUNK_SYSTEM,
                    if standards.is_empty() {
                        String::new()
                    } else {
                        format!("Project standards:\n{}", standards)
                    }
                ),
            },
            ChatMessage {
                role: ChatRole::User,
                content: context_note,
            },
        ];
        if !file_ctx.is_empty() {
            messages.push(ChatMessage {
                role: ChatRole::User,
                content: file_ctx,
            });
        }
        messages.push(ChatMessage {
            role: ChatRole::User,
            content: user_msg,
        });
        let _permit = llm_permits
            .acquire_owned()
            .await
            .map_err(|_| AppError::Ai("LLM concurrency limiter closed".into()))?;
        chat_with_retries(&provider, &messages, retry_count).await
    }
}

/// Calls `provider.chat_with_model` up to `1 + retries` times (initial attempt
/// plus retries). With `retries = 0`, makes a single attempt — important for
/// slow local providers where a "failure" is usually just a request the
/// engine's request_timeout fired on, while the model is still generating;
/// retrying just adds another orphaned in-flight request.
async fn chat_with_retries_and_model(
    provider: &Arc<dyn AiProvider>,
    messages: &[ChatMessage],
    model_override: Option<&str>,
    retries: u32,
) -> Result<String, AppError> {
    let mut last_err: Option<AppError> = None;
    for attempt in 0..=retries {
        if attempt > 0 {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
        match provider.chat_with_model(messages, model_override).await {
            Ok(r) => return Ok(r),
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap_or_else(|| AppError::Ai("Chat failed with no error info".into())))
}

/// Reorder `entries` so their paths follow `order`, returning `true` on success.
///
/// Resume is positional: the persisted state addresses files by
/// `current_file_idx` into the worklist, so a resumed run must rebuild that
/// worklist in the exact order the state was saved against. We can't, because
/// the order is derived fresh each run and isn't stable: `fetch_file_inputs`
/// emits cache hits before cache misses, so a cold first run and a warm resume
/// reorder equal-hunk-count files differently. Realigning the freshly built
/// entries to the saved order restores the slot-for-slot correspondence.
///
/// Succeeds only when `entries` and `order` describe exactly the same set of
/// paths — the precondition for a coherent resume. On any divergence (a file
/// added, removed, or renamed since the save) it returns `false` and leaves
/// `entries` untouched so the caller can fall back to a fresh run.
fn align_to_saved_order(
    entries: &mut Vec<(FileInput, Vec<crate::diff::engine::DiffHunk>)>,
    order: &[String],
) -> bool {
    if entries.len() != order.len() {
        return false;
    }
    let mut by_path: std::collections::HashMap<&str, usize> =
        std::collections::HashMap::with_capacity(entries.len());
    for (i, (f, _)) in entries.iter().enumerate() {
        by_path.insert(f.path.as_str(), i);
    }
    // Resolve each saved path to a current entry, refusing to reuse one so a
    // duplicate (or a path missing from the current set) bails cleanly.
    let mut picks = Vec::with_capacity(order.len());
    let mut used = vec![false; entries.len()];
    for p in order {
        match by_path.get(p.as_str()) {
            Some(&i) if !used[i] => {
                used[i] = true;
                picks.push(i);
            }
            _ => return false,
        }
    }
    let mut slots: Vec<Option<(FileInput, Vec<crate::diff::engine::DiffHunk>)>> =
        entries.drain(..).map(Some).collect();
    *entries = picks
        .into_iter()
        // Each index is used exactly once (guarded by `used`), so `take` is Some.
        .map(|i| slots[i].take().expect("entry index reused"))
        .collect();
    true
}

async fn save_state_to_db(conn: &libsql::Connection, state: &ReviewState) {
    let _ = state::save_state(conn, state).await;
}

async fn clear_state_from_db(conn: &libsql::Connection) {
    let _ = state::clear_state(conn).await;
}

/// Post review findings to ADO as PR comments.
pub async fn post_findings(
    findings: &[Finding],
    summary: &str,
    project_id: &str,
    repo_id: &str,
    pr_id: i64,
    client: &crate::provider::GitClient,
) -> Result<(), AppError> {
    // Post the overall summary as a PR-level thread
    let summary_thread = serde_json::json!({
        "comments": [{
            "parentCommentId": 0,
            "content": summary,
        }],
        "status": "active",
    });
    client
        .post_thread(project_id, repo_id, pr_id, &summary_thread)
        .await?;

    // Triage split: actionable findings (Blocking, Should-fix) are PULLED
    // FORWARD as their own comments; low-priority findings (Nit, FYI) are
    // PUSHED BACK into a single rollup comment so they never bury the signal.
    // `findings` arrives already ordered by tier, so blocking issues post first.
    let mut pushed_back: Vec<&Finding> = Vec::new();

    for finding in findings {
        if finding.comment.trim().is_empty() {
            continue;
        }
        if !finding.tier.is_actionable() {
            pushed_back.push(finding);
            continue;
        }
        post_single_finding(client, project_id, repo_id, pr_id, finding).await?;
    }

    // One rollup comment for everything pushed back.
    if !pushed_back.is_empty() {
        let rollup = build_rollup_comment(&pushed_back);
        let thread = serde_json::json!({
            "comments": [{ "parentCommentId": 0, "content": rollup, "commentType": 1 }],
            "status": "active",
        });
        client
            .post_thread(project_id, repo_id, pr_id, &thread)
            .await?;
    }

    Ok(())
}

/// Build the single "pushed back" rollup comment listing low-priority findings,
/// so they live in one place instead of N individual threads.
fn build_rollup_comment(findings: &[&Finding]) -> String {
    let mut out = format!(
        "## ⚪ Lower-priority findings ({})\n_Grouped into one comment to keep the review focused on higher-priority issues._\n\n",
        findings.len()
    );
    for f in findings {
        let loc = match (f.line_start, f.line_end) {
            (Some(lo), Some(hi)) if hi != lo => format!("{}:{}-{}", f.file_path, lo, hi),
            (Some(lo), _) => format!("{}:{}", f.file_path, lo),
            _ => f.file_path.clone(),
        };
        out.push_str(&format!(
            "- **{}** ({}) — {}\n",
            loc,
            f.tier.label(),
            f.comment.trim()
        ));
    }
    out
}

fn tier_prefix(t: Tier) -> &'static str {
    match t {
        Tier::Blocking => "🔴 BLOCKING —",
        Tier::ShouldFix => "🟡 SHOULD FIX —",
        Tier::Nit => "⚪ NIT —",
        Tier::Fyi => "💬 FYI —",
    }
}

/// Post one finding as its own ADO thread, tier-tagged and anchored to its line
/// range when known. Shared by the full `post_findings` path and the Phase 4
/// auto-post path so formatting stays identical.
pub async fn post_single_finding(
    client: &crate::provider::GitClient,
    project_id: &str,
    repo_id: &str,
    pr_id: i64,
    finding: &Finding,
) -> Result<(), AppError> {
    let prefix = tier_prefix(finding.tier);
    let body = format!(
        "{} **{}**\n\n{}",
        prefix, finding.file_path, finding.comment
    );
    let inline = format!("{} {}", prefix, finding.comment);

    let thread = if let (Some(lo), Some(hi)) = (finding.line_start, finding.line_end) {
        let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
        serde_json::json!({
            "comments": [{ "parentCommentId": 0, "content": inline, "commentType": 1 }],
            "status": 1,
            "threadContext": {
                "filePath": if finding.file_path.starts_with('/') {
                    finding.file_path.clone()
                } else {
                    format!("/{}", finding.file_path)
                },
                "rightFileStart": { "line": lo, "offset": 1 },
                "rightFileEnd":   { "line": hi, "offset": 1 },
            },
        })
    } else {
        serde_json::json!({
            "comments": [{ "parentCommentId": 0, "content": body, "commentType": 1 }],
            "status": 1,
        })
    };

    client
        .post_thread(project_id, repo_id, pr_id, &thread)
        .await?;
    Ok(())
}

/// Phase 4: whether a PR needs an auto-review. True when auto-review is enabled
/// and the PR has a newer iteration than the last one we reviewed (or was never
/// reviewed — `last` is `None`).
pub fn should_auto_review(enabled: bool, last_reviewed: Option<i32>, current: i32) -> bool {
    enabled && current > last_reviewed.unwrap_or(0)
}

/// Phase 4: select the findings eligible for unattended auto-posting — Blocking
/// tier at or above the confidence floor. Returned in the engine's existing
/// (blocking-first, highest-confidence-first) order.
pub fn select_auto_post_findings(findings: &[Finding], confidence_floor: u8) -> Vec<&Finding> {
    findings
        .iter()
        .filter(|f| {
            f.tier == Tier::Blocking
                && f.confidence >= confidence_floor
                && !f.comment.trim().is_empty()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str, hunks: usize) -> (FileInput, Vec<crate::diff::engine::DiffHunk>) {
        (
            FileInput {
                path: path.into(),
                old_content: String::new(),
                new_content: String::new(),
            },
            (0..hunks).map(|i| hunk(i + 1, 1)).collect(),
        )
    }

    fn paths(entries: &[(FileInput, Vec<crate::diff::engine::DiffHunk>)]) -> Vec<String> {
        entries.iter().map(|(f, _)| f.path.clone()).collect()
    }

    #[test]
    fn align_reorders_to_saved_order() {
        // The cold run saved this order; the warm resume rebuilt a different one
        // (e.g. cache hits floated to the front). Realigning restores it.
        let saved = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let mut entries = vec![entry("c", 1), entry("a", 2), entry("b", 1)];
        assert!(align_to_saved_order(&mut entries, &saved));
        assert_eq!(paths(&entries), saved);
        // Hunks travel with their file, not the slot.
        assert_eq!(entries[0].1.len(), 2); // "a" still has 2 hunks
    }

    #[test]
    fn align_succeeds_for_identical_order() {
        let saved = vec!["a".to_string(), "b".to_string()];
        let mut entries = vec![entry("a", 1), entry("b", 1)];
        assert!(align_to_saved_order(&mut entries, &saved));
        assert_eq!(paths(&entries), saved);
    }

    #[test]
    fn align_fails_when_a_file_was_removed() {
        // Current set is missing "b" → indices can't be mapped; bail untouched.
        let saved = vec!["a".to_string(), "b".to_string()];
        let mut entries = vec![entry("a", 1)];
        assert!(!align_to_saved_order(&mut entries, &saved));
        assert_eq!(paths(&entries), vec!["a".to_string()]);
    }

    #[test]
    fn align_fails_when_a_file_was_added() {
        // Current set has an extra "c" not in the saved order → bail untouched.
        let saved = vec!["a".to_string(), "b".to_string()];
        let mut entries = vec![entry("a", 1), entry("b", 1), entry("c", 1)];
        assert!(!align_to_saved_order(&mut entries, &saved));
        assert_eq!(
            paths(&entries),
            vec!["a".to_string(), "b".to_string(), "c".to_string()]
        );
    }

    /// A hunk covering new-side lines [new_start, new_start+new_count).
    fn hunk(new_start: usize, new_count: usize) -> crate::diff::engine::DiffHunk {
        crate::diff::engine::DiffHunk {
            index: 0,
            header: format!("@@ -1,1 +{},{} @@", new_start, new_count),
            old_start: 1,
            old_count: 1,
            new_start,
            new_count,
            lines: Vec::new(),
        }
    }

    fn numbered_content(lines: usize) -> String {
        (1..=lines)
            .map(|line| format!("line {}", line))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn finding(confidence: u8, line_start: Option<usize>) -> FileAggregateFinding {
        FileAggregateFinding {
            severity: Severity::Moderate,
            confidence,
            line_start,
            line_end: line_start,
            comment: "x".into(),
            existing_code: line_start.map(|line| format!("line {}", line)),
            evidence: None,
            sources: vec![],
        }
    }

    fn aggregate(findings: Vec<FileAggregateFinding>) -> FileAggregateResult {
        FileAggregateResult {
            summary: "s".into(),
            verdict: "review-required".into(),
            findings,
        }
    }

    #[test]
    fn default_confidence_is_reporting_threshold() {
        assert_eq!(
            default_confidence(),
            crate::ai::DEFAULT_CONFIDENCE_THRESHOLD
        );
    }

    #[test]
    fn guard_drops_below_threshold_keeps_at_or_above() {
        let hunks = [hunk(10, 5)]; // new-side lines 10..=14
        let new_content = numbered_content(20);
        let mut agg = aggregate(vec![
            finding(79, Some(12)), // below 80 → dropped
            finding(80, Some(12)), // exactly at threshold → kept
            finding(95, Some(13)), // above → kept
        ]);
        let dropped = apply_finding_guards(&mut agg, "f.rs", 80, &hunks, &new_content);
        assert_eq!(dropped, 1);
        assert_eq!(agg.findings.len(), 2);
        assert!(agg.findings.iter().all(|f| f.confidence >= 80));
    }

    #[test]
    fn guard_rewrites_line_from_existing_code() {
        let hunks = [hunk(10, 5)];
        let new_content = numbered_content(20);
        let mut f = finding(95, Some(99));
        f.existing_code = Some("line 14".into());
        let mut agg = aggregate(vec![f]);
        let dropped = apply_finding_guards(&mut agg, "f.rs", 80, &hunks, &new_content);
        assert_eq!(dropped, 0);
        assert_eq!(agg.findings.len(), 1);
        assert_eq!(agg.findings[0].line_start, Some(14));
    }

    #[test]
    fn guard_drops_line_level_finding_without_existing_code() {
        let hunks = [hunk(10, 5)];
        let new_content = numbered_content(20);
        let mut f = finding(95, Some(12));
        f.existing_code = None;
        let mut agg = aggregate(vec![f]);
        let dropped = apply_finding_guards(&mut agg, "f.rs", 80, &hunks, &new_content);
        assert_eq!(dropped, 1);
        assert!(agg.findings.is_empty());
    }

    #[test]
    fn guard_exempts_file_level_findings_from_anchor_check() {
        let hunks = [hunk(10, 5)];
        let new_content = numbered_content(20);
        let mut agg = aggregate(vec![finding(90, None)]); // file-level, high confidence
        let dropped = apply_finding_guards(&mut agg, "f.rs", 80, &hunks, &new_content);
        assert_eq!(dropped, 0);
        assert_eq!(agg.findings.len(), 1);
    }

    #[test]
    fn guard_zero_threshold_surfaces_everything_in_range() {
        let hunks = [hunk(1, 100)];
        let new_content = numbered_content(100);
        let mut agg = aggregate(vec![finding(0, Some(5)), finding(10, Some(6))]);
        let dropped = apply_finding_guards(&mut agg, "f.rs", 0, &hunks, &new_content);
        assert_eq!(dropped, 0);
        assert_eq!(agg.findings.len(), 2);
    }

    // Default "critical line" used by most tier tests.
    const BLOCK: u8 = crate::ai::DEFAULT_BLOCKING_CONFIDENCE;

    #[test]
    fn tier_critical_is_always_actionable() {
        // Critical at/above the critical line blocks; below it is still
        // actionable (should-fix), never demoted to a nit, and never FYI even
        // without a line anchor.
        assert_eq!(
            tier_for(Severity::Critical, 90, Some(5), BLOCK),
            Tier::Blocking
        );
        assert_eq!(
            tier_for(Severity::Critical, 84, Some(5), BLOCK),
            Tier::ShouldFix
        );
        assert_eq!(
            tier_for(Severity::Critical, 99, None, BLOCK),
            Tier::Blocking
        );
        assert!(tier_for(Severity::Critical, 50, None, BLOCK).is_actionable());
    }

    #[test]
    fn tier_critical_line_is_configurable() {
        // A confidence-80 critical is should-fix at the default line (85) but
        // blocking once the line is lowered to 80; raising it past the score
        // pushes it back to should-fix.
        assert_eq!(
            tier_for(Severity::Critical, 80, Some(5), 85),
            Tier::ShouldFix
        );
        assert_eq!(
            tier_for(Severity::Critical, 80, Some(5), 80),
            Tier::Blocking
        );
        assert_eq!(
            tier_for(Severity::Critical, 80, Some(5), 95),
            Tier::ShouldFix
        );
        // A line of 0 makes every critical finding block.
        assert_eq!(tier_for(Severity::Critical, 1, Some(5), 0), Tier::Blocking);
    }

    #[test]
    fn tier_moderate_splits_on_confidence_and_anchor() {
        assert_eq!(
            tier_for(Severity::Moderate, 85, Some(5), BLOCK),
            Tier::ShouldFix
        );
        assert_eq!(tier_for(Severity::Moderate, 79, Some(5), BLOCK), Tier::Nit);
        // Non-critical with no line anchor is informational.
        assert_eq!(tier_for(Severity::Moderate, 95, None, BLOCK), Tier::Fyi);
    }

    #[test]
    fn tier_minor_is_nit_or_fyi() {
        assert_eq!(tier_for(Severity::Minor, 100, Some(5), BLOCK), Tier::Nit);
        assert_eq!(tier_for(Severity::Minor, 100, None, BLOCK), Tier::Fyi);
        assert!(!tier_for(Severity::Minor, 100, Some(5), BLOCK).is_actionable());
    }

    #[test]
    fn tier_rank_orders_blocking_first() {
        assert!(Tier::Blocking.rank() < Tier::ShouldFix.rank());
        assert!(Tier::ShouldFix.rank() < Tier::Nit.rank());
        assert!(Tier::Nit.rank() < Tier::Fyi.rank());
    }

    #[test]
    fn rollup_lists_each_pushed_back_finding() {
        let nit = Finding {
            file_path: "a.rs".into(),
            severity: Severity::Minor,
            confidence: 90,
            tier: Tier::Nit,
            sources: vec![],
            line_start: Some(3),
            line_end: Some(3),
            comment: "rename x".into(),
        };
        let fyi = Finding {
            file_path: "b.rs".into(),
            severity: Severity::Moderate,
            confidence: 88,
            tier: Tier::Fyi,
            sources: vec![],
            line_start: None,
            line_end: None,
            comment: "consider a test file".into(),
        };
        let body = build_rollup_comment(&[&nit, &fyi]);
        assert!(body.contains("Lower-priority findings (2)"));
        assert!(body.contains("a.rs:3"));
        assert!(body.contains("(Nit)"));
        assert!(body.contains("b.rs"));
        assert!(body.contains("(FYI)"));
    }

    #[test]
    fn should_auto_review_respects_enabled_and_iteration() {
        assert!(
            !should_auto_review(false, None, 3),
            "disabled never triggers"
        );
        assert!(
            should_auto_review(true, None, 1),
            "never-reviewed PR triggers"
        );
        assert!(
            should_auto_review(true, Some(2), 3),
            "newer iteration triggers"
        );
        assert!(
            !should_auto_review(true, Some(3), 3),
            "same iteration does not"
        );
        assert!(
            !should_auto_review(true, Some(4), 3),
            "older current does not"
        );
    }

    #[test]
    fn auto_post_selects_only_high_confidence_blocking() {
        let mk = |tier: Tier, confidence: u8, comment: &str| Finding {
            file_path: "a.rs".into(),
            severity: Severity::Critical,
            confidence,
            tier,
            sources: vec![],
            line_start: Some(1),
            line_end: Some(1),
            comment: comment.into(),
        };
        let findings = vec![
            mk(Tier::Blocking, 95, "post me"),
            mk(Tier::Blocking, 89, "below floor"),
            mk(Tier::ShouldFix, 99, "not blocking"),
            mk(Tier::Blocking, 92, "  "), // empty comment
        ];
        let selected = select_auto_post_findings(&findings, 90);
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].comment, "post me");
    }

    #[test]
    fn aggregate_parses_confidence_and_evidence() {
        let raw = r#"{
          "summary": "s", "verdict": "needs-work",
          "findings": [{"severity":"critical","confidence":92,"lineStart":3,"lineEnd":3,"evidence":"line 3","comment":"boom"}]
        }"#;
        let parsed = parse_file_aggregate(raw).expect("parse");
        assert_eq!(parsed.findings[0].confidence, 92);
        assert_eq!(parsed.findings[0].evidence.as_deref(), Some("line 3"));
    }

    #[test]
    fn aggregate_defaults_missing_confidence_and_evidence() {
        // A pre-confidence aggregate (no confidence / evidence fields) must
        // still deserialize, defaulting confidence to the reporting threshold
        // so legacy findings are not silently dropped.
        let raw = r#"{
          "summary": "s", "verdict": "approve",
          "findings": [{"severity":"minor","lineStart":1,"lineEnd":1,"comment":"nit"}]
        }"#;
        let parsed = parse_file_aggregate(raw).expect("parse");
        assert_eq!(parsed.findings[0].confidence, default_confidence());
        assert!(parsed.findings[0].evidence.is_none());
    }
}
