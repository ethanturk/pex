use crate::ai::{AiProvider, ChatMessage, ChatRole};
use crate::diff::engine::extract_hunks;
use crate::review::prompts;
use crate::review::state::{self, ReviewState};
use crate::AppError;
use std::sync::Arc;
use tauri::Emitter;

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
}

#[derive(Debug, Clone)]
pub struct FileInput {
    pub path: String,
    pub old_content: String,
    pub new_content: String,
}

/// A single review finding produced by the engine.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Finding {
    pub file_path: String,
    pub new_lineno: Option<usize>,
    pub content: String,
}

/// The complete review output.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ReviewOutput {
    pub summary: String,
    pub findings: Vec<Finding>,
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
    let mut state = ReviewState::new(input.pr_key.clone(), file_paths.clone());

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
            let hunk = &hunks[state.current_hunk];

            let context_note = prompts::hunk_context_note(
                &file.path,
                state.current_hunk + 1,
                total_hunks,
            );
            messages.push(ChatMessage {
                role: ChatRole::User,
                content: context_note,
            });

            let hunk_text: String = hunk
                .lines
                .iter()
                .map(|l| format!("{}{}", l.kind, l.content))
                .collect::<Vec<_>>()
                .join("");

            let user_msg = prompts::hunk_user_message(
                &file.path,
                &hunk.header,
                &hunk_text,
                "",
            );
            messages.push(ChatMessage {
                role: ChatRole::User,
                content: user_msg,
            });

            let response = match retry_once(&provider, &messages).await {
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

            messages.push(ChatMessage {
                role: ChatRole::Assistant,
                content: response.clone(),
            });

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

            let summary = retry_once(&provider, &agg_messages).await.unwrap_or_else(|e| {
                format!("[aggregate failed — {}]", e)
            });

            state.completed_files.push((file.path.clone(), summary));
        } else {
            state.completed_files.push((file.path.clone(), "No issues found in this file.".into()));
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
        let start = (state.current_batch - 1) * batch_size;
        let end = (start + batch_size).min(state.completed_files.len());

        if start >= state.completed_files.len() {
            break;
        }

        let batch_files: Vec<(String, String)> = state.completed_files[start..end].to_vec();

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

        let batch_summary = retry_once(&provider, &batch_messages)
            .await
            .unwrap_or_else(|e| format!("[batch aggregate failed — {}]", e));

        state.batch_summaries.push(batch_summary);
        state.current_batch += 1;

        save_state_to_db(db, &state);
    }

    // ---- Phase 3: Final Synthesis ----
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

    let final_review = retry_once(&provider, &final_messages)
        .await
        .unwrap_or_else(|e| format!("[final synthesis failed — {}]", e));

    let findings: Vec<Finding> = state
        .completed_files
        .iter()
        .flat_map(|(file_path, summary)| {
            vec![Finding {
                file_path: file_path.clone(),
                new_lineno: None,
                content: summary.clone(),
            }]
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

async fn retry_once(
    provider: &Arc<dyn AiProvider>,
    messages: &[ChatMessage],
) -> Result<String, AppError> {
    match provider.chat(messages).await {
        Ok(r) => Ok(r),
        Err(_e) => {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            provider.chat(messages).await
        }
    }
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

    // Post per-file findings as threaded comments
    for finding in findings {
        if finding.content.trim().is_empty()
            || finding.content == "No issues found in this file."
            || finding.content.starts_with("[aggregate failed")
        {
            continue;
        }

        let thread = serde_json::json!({
            "comments": [{
                "parentCommentId": 0,
                "content": format!("**{}**\n\n{}", finding.file_path, finding.content),
            }],
            "status": "active",
        });

        client
            .post_thread(project_id, repo_id, pr_id, &thread)
            .await?;
    }

    Ok(())
}
