pub mod ado;
pub mod ai;
pub mod auth;
pub mod cache;
pub mod commands;
pub mod diff;
pub mod purist;
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
    pub purist_pid: std::sync::Arc<std::sync::Mutex<Option<u32>>>,
    pub diff_cache: cache::diff_cache::DiffCache,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let db = cache::init_db().expect("Failed to initialize database");

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
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
            purist_pid: std::sync::Arc::new(std::sync::Mutex::new(None)),
            diff_cache: cache::diff_cache::DiffCache::new(),
        })
        .invoke_handler(tauri::generate_handler![
            commands::auth::login_pat,
            commands::auth::login_oauth,
            commands::auth::refresh_oauth_token,
            commands::auth::get_saved_orgs,
            commands::auth::remove_org,
            commands::auth::activate_org,
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
            commands::comments::post_reply,
            commands::comments::update_reviewer_status,
            commands::ai::get_ai_settings,
            commands::ai::save_ai_settings,
            commands::ai::explain_diff,
            commands::ai::check_purist,
            commands::ai::get_purist_path,
            commands::ai::save_purist_path,
            commands::ai::review_pr_dry_run,
            commands::ai::review_pr_post,
            commands::ai::cancel_review,
            commands::ai::test_ai_connection,
            commands::ai::get_diff_hunks,
            commands::ai::review_hunk,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
