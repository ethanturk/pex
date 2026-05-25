use crate::AppError;
use std::process::Stdio;
use tauri::Emitter;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command as AsyncCommand;

/// Check that Purist is available at the given path.
pub fn check(purist_path: &str) -> Result<super::commands::ai::PuristCheckResult, AppError> {
    let path = std::path::Path::new(purist_path);
    if !path.exists() {
        return Ok(super::commands::ai::PuristCheckResult {
            ok: false,
            message: format!("Path not found: {}", purist_path),
        });
    }

    let csproj = path.join("src").join("Purist").join("Purist.csproj");
    if !csproj.exists() {
        return Ok(super::commands::ai::PuristCheckResult {
            ok: false,
            message: format!(
                "Purist.csproj not found at {}. Is this a Purist repo clone?",
                csproj.display()
            ),
        });
    }

    let dotnet_path = find_dotnet();

    let output = std::process::Command::new(&dotnet_path)
        .arg("--version")
        .output()
        .map_err(|_| AppError::Ai(
            "dotnet not found. Install .NET 10 SDK: https://dotnet.microsoft.com/download".into(),
        ))?;

    if !output.status.success() {
        return Ok(super::commands::ai::PuristCheckResult {
            ok: false,
            message: "dotnet CLI is not working. Install .NET 10 SDK.".into(),
        });
    }

    let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let major: i32 = version
        .split('.')
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    if major < 10 {
        return Ok(super::commands::ai::PuristCheckResult {
            ok: false,
            message: format!(
                ".NET SDK {} found, but Purist requires .NET 10+. Install: https://dotnet.microsoft.com/download/dotnet/10.0",
                version
            ),
        });
    }

    Ok(super::commands::ai::PuristCheckResult {
        ok: true,
        message: format!("Purist found at {}. .NET {}", purist_path, version),
    })
}

/// Run Purist against a PR URL, streaming stdout via Tauri events.
/// `dry_run`: if true, passes --dry-run to Purist (review but don't post).
/// `event_prefix`: Tauri event name prefix — emits `{prefix}-chunk` and `{prefix}-done`.
pub async fn run_review(
    purist_path: &str,
    pr_url: &str,
    dry_run: bool,
    ado_pat: &str,
    llm_provider: &str,
    llm_endpoint: &str,
    llm_api_key: &str,
    llm_model: &str,
    event_prefix: &str,
    app_handle: tauri::AppHandle,
    process_holder: std::sync::Arc<std::sync::Mutex<Option<u32>>>,
) -> Result<(), AppError> {
    let dotnet = find_dotnet();
    let csproj = format!("{}/src/Purist/Purist.csproj", purist_path);

    let mut args = vec![
        "run",
        "--project",
        &csproj,
        "--",
        "-u",
        pr_url,
    ];

    if dry_run {
        args.push("--dry-run");
    }

    let mut cmd = AsyncCommand::new(&dotnet);
    cmd.args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("LLM_PROVIDER", llm_provider)
        .env("LLM_ENDPOINT", llm_endpoint)
        .env("LLM_API_KEY", llm_api_key)
        .env("LLM_MODEL", llm_model)
        .env("ADO_PAT", ado_pat)
        .env("DOTNET_CLI_TELEMETRY_OPTOUT", "1")
        .kill_on_drop(true);

    let mut child = cmd
        .spawn()
        .map_err(|e| AppError::Ai(format!("Failed to start Purist: {}", e)))?;

    let child_id = child.id().expect("Purist process has no PID");

    // Store PID for cancellation
    {
        let mut holder = process_holder.lock().unwrap();
        *holder = Some(child_id);
    }

    let stdout = child.stdout.take().ok_or_else(|| {
        AppError::Ai("Failed to capture Purist stdout".into())
    })?;

    let stderr = child.stderr.take();

    // Spawn a task to emit events — this runs concurrently with the process
    let handle = app_handle.clone();
    let prefix = event_prefix.to_string();
    let stdout_task = tokio::spawn(async move {
        let mut reader = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            let _ = handle.emit(
                &format!("{}-chunk", prefix),
                serde_json::json!({"text": line}),
            );
        }
    });

    // Also capture stderr
    let stderr_buf = if let Some(stderr) = stderr {
        let handle = app_handle.clone();
        let prefix2 = event_prefix.to_string();
        let stderr_task = tokio::spawn(async move {
            let mut reader = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                let _ = handle.emit(
                    &format!("{}-chunk", prefix2),
                    serde_json::json!({"text": format!("[stderr] {}", line)}),
                );
            }
        });
        Some(stderr_task)
    } else {
        None
    };

    // Wait for stdout to finish
    stdout_task.await.map_err(|e| {
        AppError::Ai(format!("Purist stdout reader failed: {}", e))
    })?;

    // Wait for the process to exit
    let status = child.wait().await.map_err(|e| {
        AppError::Ai(format!("Purist process error: {}", e))
    })?;

    // Clean up stderr
    if let Some(t) = stderr_buf {
        t.abort();
    }

    let success = status.success();
    let exit_msg = if success {
        "Purist completed successfully".to_string()
    } else {
        format!("Purist exited with code {}", status.code().unwrap_or(-1))
    };

    // Clear PID
    {
        let mut holder = process_holder.lock().unwrap();
        *holder = None;
    }

    let _ = app_handle.emit(
        &format!("{}-done", event_prefix),
        serde_json::json!({"success": success, "message": exit_msg}),
    );

    Ok(())
}

/// Cancel a running Purist review by killing the process.
pub fn cancel(pid: u32) {
    #[cfg(unix)]
    {
        let _ = std::process::Command::new("kill")
            .arg("-TERM")
            .arg(pid.to_string())
            .output();
    }
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/F"])
            .output();
    }
}

/// Find the dotnet executable — check PATH, then ~/.dotnet/dotnet (dotnet-install.sh default).
fn find_dotnet() -> String {
    if std::process::Command::new("dotnet")
        .arg("--version")
        .output()
        .is_ok()
    {
        return "dotnet".to_string();
    }

    let home = std::env::var("HOME").unwrap_or_default();
    let candidate = format!("{}/.dotnet/dotnet", home);
    if std::path::Path::new(&candidate).exists() {
        return candidate;
    }

    "dotnet".to_string()
}
