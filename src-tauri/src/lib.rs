pub mod ado;
pub mod auth;
pub mod cache;
pub mod commands;
pub mod diff;
pub mod window_state;

use thiserror::Error;

use tauri::Manager;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("ADO API error: {0}")]
    Ado(String),
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
