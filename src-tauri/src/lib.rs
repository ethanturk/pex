pub mod ado;
pub mod ai;
pub mod auth;
pub mod cache;
pub mod commands;
pub mod db;
pub mod diff;
pub mod github;
pub mod provider;
pub mod review;
pub mod window_state;

use thiserror::Error;

#[cfg(not(any(target_os = "linux", mobile)))]
use tauri::Manager;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("Provider API error: {0}")]
    Provider(String),
    #[error("AI error: {0}")]
    Ai(String),
    #[error("Auth error: {0}")]
    Auth(String),
    #[error("Database error: {0}")]
    Db(#[from] libsql::Error),
    #[error("Storage error: {0}")]
    Storage(String),
    #[cfg(not(target_os = "android"))]
    #[error("Keyring error: {0}")]
    Keyring(#[from] keyring::Error),
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
}

impl serde::Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        serializer.serialize_str(self.to_string().as_ref())
    }
}

pub struct AppState {
    pub db: db::Store,
    pub client: std::sync::Mutex<Option<provider::GitClient>>,
    pub ai_manager: std::sync::Mutex<Option<ai::AiManager>>,
    pub diff_cache: cache::diff_cache::DiffCache,
    pub standards_cache: cache::standards_cache::StandardsCache,
    /// Flipped to true by `cancel_review`; the review engine checks this
    /// between LLM calls and bails out early, so in-flight reviews actually
    /// stop instead of running to completion.
    pub review_cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = rustls::crypto::ring::default_provider().install_default();

    let store = tauri::async_runtime::block_on(db::Store::open(db::load_sync_config()))
        .expect("Failed to initialize database");
    // Periodic offline-first reconcile while sync is enabled. No-op when off.
    db::spawn_background_sync(store.clone());

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            #[cfg(not(any(target_os = "linux", mobile)))]
            {
                let window = app.get_webview_window("main").expect("no main window");
                let win = window.clone();
                // Reconcile with the remote when the window regains focus, so a
                // desktop edit shows up promptly after switching back from another
                // device. No-op when sync is disabled.
                let focus_store = {
                    use tauri::Manager;
                    app.state::<AppState>().db.clone()
                };
                window.on_window_event(move |event| match event {
                    tauri::WindowEvent::CloseRequested { .. } => {
                        window_state::save(&win);
                    }
                    tauri::WindowEvent::Focused(true) => {
                        let store = focus_store.clone();
                        tauri::async_runtime::spawn(async move {
                            if store.is_synced() {
                                let _ = store.sync_now().await;
                            }
                        });
                    }
                    _ => {}
                });
                window_state::restore(&window);
            }
            #[cfg(target_os = "android")]
            {
                use tauri::Manager;
                match app.path().app_data_dir() {
                    Ok(dir) => auth::android_keystore::init(dir),
                    Err(e) => eprintln!("pex: could not resolve app data dir: {e}"),
                }
            }
            #[cfg(any(target_os = "linux", target_os = "ios"))]
            let _ = app;
            Ok(())
        })
        .manage(AppState {
            db: store,
            client: std::sync::Mutex::new(None),
            ai_manager: std::sync::Mutex::new(None),
            diff_cache: cache::diff_cache::DiffCache::new(),
            standards_cache: cache::standards_cache::StandardsCache::new(),
            review_cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        })
        .invoke_handler(tauri::generate_handler![
            commands::auth::login_pat,
            commands::auth::login_oauth,
            commands::auth::refresh_oauth_token,
            commands::auth::get_saved_orgs,
            commands::auth::remove_org,
            commands::auth::activate_org,
            commands::auth::get_current_user_id,
            commands::pr::list_projects,
            commands::pr::list_repositories,
            commands::pr::list_pull_requests,
            commands::pr::get_pull_request,
            commands::pr::get_pr_checks,
            commands::pr::get_iterations,
            commands::files::get_pr_files,
            commands::files::get_file_diff,
            commands::files::prefetch_pr_diffs,
            commands::files::get_file_lines,
            commands::files::mark_file_viewed,
            commands::files::get_viewed_files,
            commands::comments::get_vote_history,
            commands::comments::get_threads,
            commands::comments::post_comment,
            commands::comments::post_review_finding,
            commands::comments::post_reply,
            commands::comments::update_comment,
            commands::comments::update_reviewer_status,
            commands::ai::get_ai_settings,
            commands::ai::save_ai_defaults,
            commands::ai::save_ai_provider_config,
            commands::ai::remove_ai_provider,
            commands::ai::save_ai_preferences,
            commands::ai::test_ai_defaults,
            commands::ai::explain_hunk,
            commands::ai::get_ai_prompts,
            commands::ai::get_review_specialists,
            commands::ai::save_ai_prompt,
            commands::ai::reset_ai_prompt,
            commands::ai::save_ai_prompt_model,
            commands::ai::reset_ai_prompt_model,
            commands::ai::list_ai_models,
            commands::ai::list_ai_provider_models,
            commands::ai::get_diff_hunks,
            commands::review::start_review,
            commands::review::preview_review,
            commands::review::start_review_post,
            commands::review::cancel_review,
            commands::review::get_saved_review,
            commands::review::clear_saved_review,
            commands::feedback::record_finding_verdict,
            commands::feedback::clear_finding_verdict,
            commands::feedback::get_review_calibration,
            commands::feedback::clear_review_feedback,
            commands::feedback::get_diagnostics_dir,
            commands::auto::auto_review_candidates,
            commands::auto::auto_post_review_findings,
            commands::sync::get_sync_status,
            commands::sync::enable_sync,
            commands::sync::disable_sync,
            commands::sync::sync_now,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
