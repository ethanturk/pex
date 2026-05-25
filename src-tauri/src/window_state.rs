/// Window state persistence — macOS and Windows only.
/// GTK on Linux has a Send-bound incompatibility with Tauri's window state API;
/// the window is not `Send` so it can't be used across async boundaries.
#[cfg(not(target_os = "linux"))]
mod imp {
    use serde::{Deserialize, Serialize};
    use std::fs;
    use std::path::PathBuf;
    use tauri::{Manager, PhysicalPosition, PhysicalSize, Runtime, WebviewWindow};

    #[derive(Debug, Serialize, Deserialize)]
    struct WindowState {
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        maximized: bool,
    }

    fn state_path<R: Runtime>(app: &tauri::AppHandle<R>) -> PathBuf {
        let mut path = app.path().app_config_dir().expect("no config dir");
        fs::create_dir_all(&path).ok();
        path.push("window-state.json");
        path
    }

    pub fn restore<R: Runtime>(window: &WebviewWindow<R>) {
        let path = state_path(window.app_handle());
        let state: WindowState = match fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
        {
            Some(s) => s,
            None => return,
        };

        // Restore position and size
        let _ = window.set_position(PhysicalPosition::new(state.x, state.y));
        let _ = window.set_size(PhysicalSize::new(state.width, state.height));

        // Restore maximized
        if state.maximized {
            let _ = window.maximize();
        }
    }

    pub fn save<R: Runtime>(window: &WebviewWindow<R>) {
        let path = state_path(window.app_handle());

        let position = window
            .outer_position()
            .unwrap_or(PhysicalPosition::new(0, 0));
        let size = window.outer_size().unwrap_or(PhysicalSize::new(1400, 900));
        let maximized = window.is_maximized().unwrap_or(false);

        // Don't save if maximized — next launch should restore the pre-maximized size
        if maximized {
            // Read existing state and keep it, just update maximized
            if let Ok(existing_json) = fs::read_to_string(&path) {
                if let Ok(mut existing) = serde_json::from_str::<WindowState>(&existing_json) {
                    existing.maximized = true;
                    if let Ok(json) = serde_json::to_string_pretty(&existing) {
                        let _ = fs::write(&path, json);
                    }
                }
            }
            return;
        }

        let state = WindowState {
            x: position.x,
            y: position.y,
            width: size.width,
            height: size.height,
            maximized: false,
        };

        if let Ok(json) = serde_json::to_string_pretty(&state) {
            let _ = fs::write(&path, json);
        }
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use tauri::{Runtime, WebviewWindow};

    pub fn restore<R: Runtime>(_window: &WebviewWindow<R>) {}
    pub fn save<R: Runtime>(_window: &WebviewWindow<R>) {}
}

pub use imp::*;
