use crate::ai::prompts::{resolve_prompt, PromptKey};
use crate::ai::{AiProvider, ChatMessage, ChatRole};
use crate::diff::engine::extract_hunks;
use crate::review::prompts;
use crate::review::state::{self, ReviewMode, ReviewState};
use crate::AppError;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::Emitter;
use tokio::sync::Semaphore;

fn cancelled(flag: &AtomicBool) -> Result<(), AppError> {
    if flag.load(Ordering::SeqCst) {
        Err(AppError::Ado("Review cancelled".into()))
    } else {
        Ok(())
    }
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
    /// New-side line(s) the adjudicator cited to justify the finding. Used by
    /// the deterministic anchor check and for logging; stripped before posting.
    #[serde(default)]
    pub evidence: Option<String>,
}

/// The complete review output.
#[derive(Debug, Clone, serde::Serialize)]
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
/// file aggregate:
///   1. drop findings whose `confidence` is below `threshold`;
///   2. drop line-anchored findings whose `line_start` falls outside every
///      reviewed hunk's new-side range — a strong hallucinated-line signal.
/// File-level findings (`line_start == None`) are exempt from the anchor check.
/// Returns the number of findings dropped (for logging / tests).
pub fn apply_finding_guards(
    aggregate: &mut FileAggregateResult,
    file_path: &str,
    threshold: u8,
    hunks: &[crate::diff::engine::DiffHunk],
) -> usize {
    let before = aggregate.findings.len();
    aggregate.findings.retain(|f| {
        if f.confidence < threshold {
            eprintln!(
                "[review] dropped finding in {} (confidence {} < threshold {})",
                file_path, f.confidence, threshold
            );
            return false;
        }
        if let Some(line) = f.line_start {
            if !line_in_any_hunk(line, hunks) {
                eprintln!(
                    "[review] dropped finding in {} (line {} outside any reviewed hunk — likely hallucinated)",
                    file_path, line
                );
                return false;
            }
        }
        true
    });
    before - aggregate.findings.len()
}

/// True if `line` (1-based, new-side) falls within any hunk's new-side range.
/// Hunks with no new-side lines (pure deletions, `new_count == 0`) match nothing.
fn line_in_any_hunk(line: usize, hunks: &[crate::diff::engine::DiffHunk]) -> bool {
    hunks
        .iter()
        .any(|h| h.new_count > 0 && line >= h.new_start && line < h.new_start + h.new_count)
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
pub async fn run_review(
    app: tauri::AppHandle,
    provider: Arc<dyn AiProvider>,
    input: ReviewInput,
    db: &std::sync::Mutex<rusqlite::Connection>,
    cancel: Arc<AtomicBool>,
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
    let retry_count = match db.lock() {
        Ok(c) => crate::ai::read_retry_count(&c).unwrap_or(crate::ai::DEFAULT_RETRY_COUNT),
        Err(_) => crate::ai::DEFAULT_RETRY_COUNT,
    };
    let hunk_concurrency = match db.lock() {
        Ok(c) => {
            crate::ai::read_hunk_concurrency(&c).unwrap_or(crate::ai::DEFAULT_HUNK_CONCURRENCY)
        }
        Err(_) => crate::ai::DEFAULT_HUNK_CONCURRENCY,
    }
    .max(1) as usize;
    let llm_permits = Arc::new(Semaphore::new(hunk_concurrency));

    // Minimum confidence a finding must reach to survive the deterministic
    // guard applied after each file's adjudication. Resolved once per run.
    let confidence_threshold = match db.lock() {
        Ok(c) => crate::ai::read_confidence_threshold(&c)
            .unwrap_or(crate::ai::DEFAULT_CONFIDENCE_THRESHOLD),
        Err(_) => crate::ai::DEFAULT_CONFIDENCE_THRESHOLD,
    };

    // Resolve specialist system prompts + per-specialist model overrides once for
    // the run (Thorough mode only). Resolved up front so user edits in Settings
    // take effect on the next run without restarting the app.
    //
    // Each tuple: (key, system prompt text, optional model override).
    // `None` model override means: fall back to the provider's configured model.
    let specialist_prompts: Vec<(PromptKey, String, Option<String>)> =
        if input.mode == ReviewMode::Thorough {
            let mut out = Vec::new();
            for key in PromptKey::THOROUGH_SPECIALISTS {
                let (text, model) = match db.lock() {
                    Ok(c) => {
                        let t = resolve_prompt(&c, *key)
                            .unwrap_or_else(|_| key.default_text().to_string());
                        let m = crate::ai::prompts::resolve_model(&c, *key).unwrap_or(None);
                        (t, m)
                    }
                    Err(_) => (key.default_text().to_string(), None),
                };
                out.push((*key, text, model));
            }
            out
        } else {
            Vec::new()
        };

    // Check for resumable state
    if let Ok(db_lock) = db.lock() {
        if let Ok(Some(saved)) = state::load_state(&db_lock) {
            if saved.pr_key == state.pr_key && !saved.is_done() {
                state = saved;
                emit_progress(
                    &app,
                    "resume",
                    "Resuming from saved progress...",
                    serde_json::json!({}),
                );
            }
        }
    }

    // ---- Phase 1: Hunk Review (per file) ----
    while state.current_file_idx < file_entries.len() {
        cancelled(&cancel)?;
        let (file, hunks) = &file_entries[state.current_file_idx];
        let total_hunks = hunks.len();
        // Shared once per file: each hunk pass windows a bounded slice of this
        // for surrounding context, so we clone the Arc, not the string.
        let file_new_content = Arc::new(file.new_content.clone());

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
                let standards = input.standards.clone();
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
                        save_state_to_db(db, &state);
                        continue;
                    }
                };

                if response.trim() != "No issues found." {
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

                save_state_to_db(db, &state);
            }
        }

        // ---- File Aggregate ----
        if !state.current_file_findings.is_empty() {
            state.phase = "file-aggregate".into();
            save_state_to_db(db, &state);
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
                    ),
                },
            ];

            let raw = chat_with_retries(&provider, &agg_messages, retry_count)
                .await
                .unwrap_or_else(|e| format!("[aggregate failed — {}]", e));

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

            // Deterministic precision guards: drop sub-threshold and
            // hallucinated-line findings before they reach the reviewer.
            apply_finding_guards(&mut aggregate, &file.path, confidence_threshold, hunks);

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

        state.current_file_idx += 1;
        state.current_file_hunks = 0;
        state.current_hunk = 0;
        state.current_file_findings.clear();
        state.phase = if state.current_file_idx >= file_entries.len() {
            "batch-aggregate".into()
        } else {
            "hunk-review".into()
        };

        save_state_to_db(db, &state);
    }

    // ---- Phase 2: Batch Aggregation ----
    let batch_size = 5;
    let total_batches = state.total_batches;
    if state.current_batch <= total_batches {
        state.phase = "batch-aggregate".into();
        save_state_to_db(db, &state);
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

        let batch_summary = chat_with_retries(&provider, &batch_messages, retry_count)
            .await
            .unwrap_or_else(|e| format!("[batch aggregate failed — {}]", e));

        state.batch_summaries.push(batch_summary);
        state.current_batch += 1;

        save_state_to_db(db, &state);
    }

    // ---- Phase 3: Final Synthesis ----
    cancelled(&cancel)?;
    state.phase = "synthesis".into();
    save_state_to_db(db, &state);
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
    let findings: Vec<Finding> = state
        .completed_files
        .iter()
        .flat_map(|(file_path, agg)| {
            agg.findings.iter().map(move |f| Finding {
                file_path: file_path.clone(),
                severity: f.severity,
                confidence: f.confidence,
                line_start: f.line_start,
                line_end: f.line_end,
                comment: f.comment.clone(),
            })
        })
        .collect();

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

    let final_review = chat_with_retries(&provider, &final_messages, retry_count)
        .await
        .unwrap_or_else(|e| format!("[final synthesis failed — {}]", e));
    let final_review = append_exact_statistics(&final_review, file_entries.len(), &findings);

    state.phase = "done".into();
    state.final_review = Some(final_review.clone());
    clear_state_from_db(db);

    emit_progress(
        &app,
        "done",
        "Review complete",
        serde_json::json!({
            "totalFiles": file_entries.len(),
            "findingsCount": findings.len(),
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

    let specialist_prompts: Vec<(PromptKey, String, Option<String>)> =
        if mode == ReviewMode::Thorough {
            PromptKey::THOROUGH_SPECIALISTS
                .iter()
                .map(|k| (*k, k.default_text().to_string(), None))
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
            ),
        },
    ];

    let raw = chat_with_retries(&provider, &agg_messages, retry_count).await?;
    let mut aggregate = parse_file_aggregate(&raw).map_err(AppError::Ai)?;
    apply_finding_guards(&mut aggregate, &file.path, confidence_threshold, &hunks);
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

    format!(
        "{}\n\n## Statistics\n- Files reviewed: {}\n- Issues found: {} critical, {} moderate, {} minor",
        without_stats, files_reviewed, critical, moderate, minor
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
    specialist_prompts: Vec<(PromptKey, String, Option<String>)>,
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
        for (idx, (key, sys_text, model_override)) in specialist_prompts.into_iter().enumerate() {
            let provider = provider.clone();
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

fn save_state_to_db(db: &std::sync::Mutex<rusqlite::Connection>, state: &ReviewState) {
    if let Ok(db_lock) = db.lock() {
        let _ = state::save_state(&db_lock, state);
    }
}

fn clear_state_from_db(db: &std::sync::Mutex<rusqlite::Connection>) {
    if let Ok(db_lock) = db.lock() {
        let _ = state::clear_state(&db_lock);
    }
}

/// Post review findings to ADO as PR comments.
pub async fn post_findings(
    findings: &[Finding],
    summary: &str,
    project_id: &str,
    repo_id: &str,
    pr_id: i64,
    client: &crate::ado::AdoClient,
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

    // Post each finding. Anchor to the source line range when the LLM
    // supplied one; otherwise fall back to a PR-level comment with the file
    // path bolded into the body (file-level threadContext is attempted first
    // by the dedicated `post_review_finding` command; the batch path keeps
    // things simple).
    for finding in findings {
        if finding.comment.trim().is_empty() {
            continue;
        }

        let prefix = severity_prefix(finding.severity);
        let body = format!(
            "{} **{}**\n\n{}",
            prefix, finding.file_path, finding.comment
        );

        let thread = if let (Some(lo), Some(hi)) = (finding.line_start, finding.line_end) {
            let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
            serde_json::json!({
                "comments": [{ "parentCommentId": 0, "content": finding.comment, "commentType": 1 }],
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
    }

    Ok(())
}

fn severity_prefix(s: Severity) -> &'static str {
    match s {
        Severity::Critical => "🔴 CRITICAL —",
        Severity::Moderate => "🟡 MODERATE —",
        Severity::Minor => "⚪ MINOR —",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn finding(confidence: u8, line_start: Option<usize>) -> FileAggregateFinding {
        FileAggregateFinding {
            severity: Severity::Moderate,
            confidence,
            line_start,
            line_end: line_start,
            comment: "x".into(),
            evidence: None,
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
        assert_eq!(default_confidence(), crate::ai::DEFAULT_CONFIDENCE_THRESHOLD);
    }

    #[test]
    fn guard_drops_below_threshold_keeps_at_or_above() {
        let hunks = [hunk(10, 5)]; // new-side lines 10..=14
        let mut agg = aggregate(vec![
            finding(79, Some(12)), // below 80 → dropped
            finding(80, Some(12)), // exactly at threshold → kept
            finding(95, Some(13)), // above → kept
        ]);
        let dropped = apply_finding_guards(&mut agg, "f.rs", 80, &hunks);
        assert_eq!(dropped, 1);
        assert_eq!(agg.findings.len(), 2);
        assert!(agg.findings.iter().all(|f| f.confidence >= 80));
    }

    #[test]
    fn guard_drops_line_outside_any_hunk() {
        let hunks = [hunk(10, 5)]; // new-side lines 10..=14
        let mut agg = aggregate(vec![
            finding(95, Some(9)),  // just before the hunk → dropped
            finding(95, Some(14)), // last line of the hunk → kept
            finding(95, Some(15)), // just after the hunk → dropped
        ]);
        let dropped = apply_finding_guards(&mut agg, "f.rs", 80, &hunks);
        assert_eq!(dropped, 2);
        assert_eq!(agg.findings.len(), 1);
        assert_eq!(agg.findings[0].line_start, Some(14));
    }

    #[test]
    fn guard_exempts_file_level_findings_from_anchor_check() {
        let hunks = [hunk(10, 5)];
        let mut agg = aggregate(vec![finding(90, None)]); // file-level, high confidence
        let dropped = apply_finding_guards(&mut agg, "f.rs", 80, &hunks);
        assert_eq!(dropped, 0);
        assert_eq!(agg.findings.len(), 1);
    }

    #[test]
    fn guard_zero_threshold_surfaces_everything_in_range() {
        let hunks = [hunk(1, 100)];
        let mut agg = aggregate(vec![finding(0, Some(5)), finding(10, Some(6))]);
        let dropped = apply_finding_guards(&mut agg, "f.rs", 0, &hunks);
        assert_eq!(dropped, 0);
        assert_eq!(agg.findings.len(), 2);
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
