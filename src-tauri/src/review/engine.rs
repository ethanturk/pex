use crate::ai::{AiProvider, ChatMessage, ChatRole};
use crate::ai::prompts::{resolve_prompt, PromptKey};
use crate::diff::engine::extract_hunks;
use crate::review::prompts;
use crate::review::state::{self, ReviewMode, ReviewState};
use crate::AppError;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::Emitter;

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

/// A single review finding produced by the engine. Each finding is intended to
/// become one ADO comment, anchored to a line range when possible.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    pub file_path: String,
    pub severity: Severity,
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
    pub line_start: Option<usize>,
    pub line_end: Option<usize>,
    pub comment: String,
}

/// The complete review output.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ReviewOutput {
    pub summary: String,
    pub findings: Vec<Finding>,
}

/// Best-effort JSON extraction from an LLM response. Strips ``` fences and
/// trims surrounding prose; returns the parsed result or an error string.
fn parse_file_aggregate(raw: &str) -> Result<FileAggregateResult, String> {
    let trimmed = raw.trim();
    // Strip leading/trailing code fences if the model ignored "no fences".
    let inner = if let Some(stripped) = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
    {
        stripped.trim_start_matches('\n').trim_end_matches("```").trim()
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

    // Resolve specialist system prompts + per-specialist model overrides once for
    // the run (Thorough mode only). Resolved up front so user edits in Settings
    // take effect on the next run without restarting the app.
    //
    // Each tuple: (key, system prompt text, optional model override).
    // `None` model override means: fall back to the provider's configured model.
    let specialist_prompts: Vec<(PromptKey, String, Option<String>)> = if input.mode == ReviewMode::Thorough {
        let mut out = Vec::new();
        for key in PromptKey::THOROUGH_SPECIALISTS {
            let (text, model) = match db.lock() {
                Ok(c) => {
                    let t = resolve_prompt(&c, *key).unwrap_or_else(|_| key.default_text().to_string());
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
                emit_progress(&app, "resume", "Resuming from saved progress...", serde_json::json!({}));
            }
        }
    }

    // ---- Phase 1: Hunk Review (per file) ----
    while state.current_file_idx < file_entries.len() {
        cancelled(&cancel)?;
        let (file, hunks) = &file_entries[state.current_file_idx];
        let total_hunks = hunks.len();

        if state.current_file_hunks == 0 {
            state.current_file_hunks = total_hunks;
            state.current_hunk = 0;
            state.current_file_findings.clear();
        }

        emit_progress(
            &app,
            "hunk-review",
            &format!("{} ({}/{})", file.path, state.current_file_idx + 1, file_entries.len()),
            serde_json::json!({
                "fileNum": state.current_file_idx + 1,
                "totalFiles": file_entries.len(),
                "hunk": state.current_hunk,
                "totalHunks": total_hunks,
            }),
        );

        let mut messages: Vec<ChatMessage> = vec![ChatMessage {
            role: ChatRole::System,
            content: format!(
                "{}\n\n{}",
                prompts::REVIEW_HUNK_SYSTEM,
                if input.standards.is_empty() {
                    String::new()
                } else {
                    format!("Project standards:\n{}", input.standards)
                }
            ),
        }];

        while state.current_hunk < total_hunks {
            cancelled(&cancel)?;
            let hunk = &hunks[state.current_hunk];

            let hunk_text: String = hunk
                .lines
                .iter()
                .map(|l| format!("{}{}", l.kind, l.content))
                .collect::<Vec<_>>()
                .join("");

            let context_note = prompts::hunk_context_note(
                &file.path,
                state.current_hunk + 1,
                total_hunks,
            );
            let user_msg = prompts::hunk_user_message(
                &file.path,
                &hunk.header,
                &hunk_text,
                "",
            );

            // Either run the single generalist pass (Fast) or fan out to all
            // specialists (Thorough). For Thorough, each specialist gets a
            // fresh message list so prompts don't bleed across passes.
            let combined_response: Result<String, AppError> = if input.mode == ReviewMode::Thorough {
                let mut outputs: Vec<String> = Vec::new();
                let mut last_err: Option<AppError> = None;
                for (key, sys_text, model_override) in &specialist_prompts {
                    let pass_messages = vec![
                        ChatMessage {
                            role: ChatRole::System,
                            content: if input.standards.is_empty() {
                                sys_text.clone()
                            } else {
                                format!("{}\n\nProject standards:\n{}", sys_text, input.standards)
                            },
                        },
                        ChatMessage {
                            role: ChatRole::User,
                            content: context_note.clone(),
                        },
                        ChatMessage {
                            role: ChatRole::User,
                            content: user_msg.clone(),
                        },
                    ];
                    match chat_with_retries_and_model(&provider, &pass_messages, model_override.as_deref(), retry_count).await {
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
                messages.push(ChatMessage {
                    role: ChatRole::User,
                    content: context_note.clone(),
                });
                messages.push(ChatMessage {
                    role: ChatRole::User,
                    content: user_msg.clone(),
                });
                let r = chat_with_retries(&provider, &messages, retry_count).await;
                if let Ok(ref response) = r {
                    messages.push(ChatMessage {
                        role: ChatRole::Assistant,
                        content: response.clone(),
                    });
                }
                r
            };

            let response = match combined_response {
                Ok(r) => r,
                Err(e) => {
                    let skip_msg = format!("[skipped — error: {}]", e);
                    emit_progress(
                        &app,
                        "hunk-skipped",
                        &format!("Hunk {}/{} in {} failed: {}", state.current_hunk + 1, total_hunks, file.path, e),
                        serde_json::json!({}),
                    );
                    state.current_file_findings.push((state.current_hunk + 1, skip_msg));
                    state.current_hunk += 1;
                    messages.truncate(1);
                    continue;
                }
            };

            if response.trim() != "No issues found." {
                state.current_file_findings.push((state.current_hunk + 1, response));
            }

            state.current_hunk += 1;

            emit_progress(
                &app,
                "hunk-review",
                &format!("{} ({}/{})", file.path, state.current_file_idx + 1, file_entries.len()),
                serde_json::json!({
                    "fileNum": state.current_file_idx + 1,
                    "totalFiles": file_entries.len(),
                    "hunk": state.current_hunk,
                    "totalHunks": total_hunks,
                }),
            );

            save_state_to_db(db, &state);
        }

        // ---- File Aggregate ----
        if !state.current_file_findings.is_empty() {
            emit_progress(
                &app,
                "file-aggregate",
                &format!("Summarizing {}", file.path),
                serde_json::json!({}),
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
                    ),
                },
            ];

            let raw = chat_with_retries(&provider, &agg_messages, retry_count).await.unwrap_or_else(|e| {
                format!("[aggregate failed — {}]", e)
            });

            let aggregate = parse_file_aggregate(&raw).unwrap_or_else(|err| {
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

        save_state_to_db(db, &state);
    }

    // ---- Phase 2: Batch Aggregation ----
    let batch_size = 5;
    let total_batches = state.total_batches;

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
            &format!("Batch {}/{} ({} files)", state.current_batch, total_batches, batch_files.len()),
            serde_json::json!({
                "batch": state.current_batch,
                "totalBatches": total_batches,
                "fileCount": batch_files.len(),
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
    emit_progress(
        &app,
        "synthesis",
        "Producing final review summary...",
        serde_json::json!({}),
    );

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

    // Flatten per-file findings into a single list, injecting the file path
    // onto each one so the frontend can render and post them independently.
    let findings: Vec<Finding> = state
        .completed_files
        .iter()
        .flat_map(|(file_path, agg)| {
            agg.findings.iter().map(move |f| Finding {
                file_path: file_path.clone(),
                severity: f.severity,
                line_start: f.line_start,
                line_end: f.line_end,
                comment: f.comment.clone(),
            })
        })
        .collect();

    state.phase = "done".into();
    state.final_review = Some(final_review.clone());
    save_state_to_db(db, &state);

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

async fn chat_with_retries(
    provider: &Arc<dyn AiProvider>,
    messages: &[ChatMessage],
    retries: u32,
) -> Result<String, AppError> {
    chat_with_retries_and_model(provider, messages, None, retries).await
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
        let body = format!("{} **{}**\n\n{}", prefix, finding.file_path, finding.comment);

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
