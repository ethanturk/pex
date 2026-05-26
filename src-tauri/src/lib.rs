pub mod ado;
pub mod ai;
pub mod auth;
pub mod cache;
pub mod commands;
pub mod diff;
pub mod review;
pub mod window_state;

use thiserror::Error;

use tauri::Manager;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("ADO API error: {0}")]
    Ado(String),
    #[error("AI error: {0}")]
    Ai(String),
    #[error("Auth error: {0}")]
    Auth(String),
    #[error("Cache error: {0}")]
    Cache(#[from] rusqlite::Error),
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
    pub db: std::sync::Mutex<rusqlite::Connection>,
    pub ado_client: std::sync::Mutex<Option<ado::AdoClient>>,
    pub ai_manager: std::sync::Mutex<Option<ai::AiManager>>,
    pub diff_cache: cache::diff_cache::DiffCache,
    pub standards_cache: cache::standards_cache::StandardsCache,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let db = cache::init_db().expect("Failed to initialize database");

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            let window = app.get_webview_window("main").expect("no main window");
            #[cfg(not(target_os = "linux"))]
            {
                let win = window.clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { .. } = event {
                        window_state::save(&win);
                    }
                });
                window_state::restore(&window);
            }
            #[cfg(target_os = "linux")]
            let _ = &window;
            Ok(())
        })
        .manage(AppState {
            db: std::sync::Mutex::new(db),
            ado_client: std::sync::Mutex::new(None),
            ai_manager: std::sync::Mutex::new(None),
            diff_cache: cache::diff_cache::DiffCache::new(),
            standards_cache: cache::standards_cache::StandardsCache::new(),
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
            commands::pr::get_iterations,
            commands::files::get_pr_files,
            commands::files::get_file_diff,
            commands::files::get_file_lines,
            commands::files::mark_file_viewed,
            commands::files::get_viewed_files,
            commands::comments::get_threads,
            commands::comments::post_comment,
            commands::comments::post_review_finding,
            commands::comments::post_reply,
            commands::comments::update_reviewer_status,
            commands::ai::get_ai_settings,
            commands::ai::save_ai_settings,
            commands::ai::explain_hunk,
            commands::ai::get_ai_prompts,
            commands::ai::save_ai_prompt,
            commands::ai::reset_ai_prompt,
            commands::ai::test_ai_connection,
            commands::ai::get_diff_hunks,
            commands::ai::review_hunk,
            commands::review::start_review,
            commands::review::start_review_post,
            commands::review::cancel_review,
            commands::review::get_saved_review,
            commands::review::clear_saved_review,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
